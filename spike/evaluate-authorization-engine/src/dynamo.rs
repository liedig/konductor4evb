use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Mutex;

use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client;

use crate::adapter::AdapterError;
use crate::contract::{
    Action, AuthContext, AuthorizationSubject, Decision, DenyReason, PrincipalType, Resource,
};
use crate::manifest::{GrantSeed, SubscriptionSeed};
use crate::store::{binding_sort_key, entitlement_sort_key, principal_key};

/// DynamoDB-backed grant adapter. Uses strongly-consistent single-key reads
/// (GetItem / key-condition Query) for every decision path — no Scan calls.
pub struct DynamoGrantAdapter {
    client: Client,
    grant_table: String,
    subscription_table: String,
    trusted_issuer: String,
    seen_commands: Mutex<HashSet<String>>,
}

impl DynamoGrantAdapter {
    pub fn new(
        client: Client,
        grant_table: impl Into<String>,
        subscription_table: impl Into<String>,
        trusted_issuer: impl Into<String>,
    ) -> Self {
        Self {
            client,
            grant_table: grant_table.into(),
            subscription_table: subscription_table.into(),
            trusted_issuer: trusted_issuer.into(),
            seen_commands: Mutex::new(HashSet::new()),
        }
    }

    fn check_issuer(&self, subject: &AuthorizationSubject, context: &AuthContext) -> Option<DenyReason> {
        if subject.kind != PrincipalType::CognitoGroup {
            return None;
        }
        let trusted = context.trusted_issuer.as_deref().unwrap_or(&self.trusted_issuer);
        if subject.issuer.as_deref() != Some(trusted) {
            return Some(DenyReason::UntrustedIssuer);
        }
        None
    }

    async fn get_binding(
        &self,
        subject: &AuthorizationSubject,
        role: &str,
        domain: &str,
    ) -> Result<bool, AdapterError> {
        let pk = principal_key(subject);
        let sk = binding_sort_key(role, domain);
        let resp = self
            .client
            .get_item()
            .table_name(&self.grant_table)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S(sk))
            .consistent_read(true)
            .send()
            .await
            .map_err(|e| AdapterError::Decision(e.to_string()))?;
        Ok(resp
            .item
            .map(|item| {
                item.get("state")
                    .and_then(|v| v.as_s().ok())
                    .is_some_and(|s| s.as_str() == "active")
            })
            .unwrap_or(false))
    }

    async fn query_entitlements(
        &self,
        consumer: &str,
        producer: &str,
    ) -> Result<Vec<(Option<String>, String, String)>, AdapterError> {
        let pk = format!("CONSUMER#{consumer}");
        let sk_prefix = format!("ENT#PRODUCER#{producer}");
        let resp = self
            .client
            .query()
            .table_name(&self.grant_table)
            .key_condition_expression("pk = :pk AND begins_with(sk, :sk)")
            .expression_attribute_values(":pk", AttributeValue::S(pk))
            .expression_attribute_values(":sk", AttributeValue::S(sk_prefix))
            .consistent_read(true)
            .send()
            .await
            .map_err(|e| AdapterError::Decision(e.to_string()))?;
        let mut result = Vec::new();
        for item in resp.items() {
            let schema = item.get("schema").and_then(|v| v.as_s().ok()).map(|s| s.as_str().to_string());
            let Some(scope) = item.get("scope").and_then(|v| v.as_s().ok()).map(|s| s.as_str().to_string()) else {
                continue;
            };
            let Some(state) = item.get("state").and_then(|v| v.as_s().ok()).map(|s| s.as_str().to_string()) else {
                continue;
            };
            result.push((schema, scope, state));
        }
        Ok(result)
    }

    async fn has_coverage(
        &self,
        consumer: &str,
        producer: &str,
        schema: &str,
    ) -> Result<bool, AdapterError> {
        let entitlements = self.query_entitlements(consumer, producer).await?;
        Ok(entitlements.into_iter().any(|(ent_schema, _scope, state)| {
            state == "active" && (ent_schema.is_none() || ent_schema.as_deref() == Some(schema))
        }))
    }

    async fn schema_exists(&self, producer: &str, schema: &str) -> Result<bool, AdapterError> {
        let pk = format!("SCHEMA#{producer}");
        let sk = format!("SCHEMA#{schema}");
        let resp = self
            .client
            .get_item()
            .table_name(&self.grant_table)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S(sk))
            .consistent_read(true)
            .send()
            .await
            .map_err(|e| AdapterError::Decision(e.to_string()))?;
        Ok(resp.item.is_some())
    }

    async fn clear_tables(&self) -> Result<(), AdapterError> {
        use aws_sdk_dynamodb::types::{DeleteRequest, WriteRequest};
        for (table, key_kind) in [(&self.grant_table, "grant"), (&self.subscription_table, "sub")] {
            loop {
                let resp = self
                    .client
                    .scan()
                    .table_name(table)
                    .consistent_read(true)
                    .send()
                    .await
                    .map_err(|e| AdapterError::Seed(e.to_string()))?;
                let items: Vec<HashMap<String, AttributeValue>> = resp.items().to_vec();
                if items.is_empty() {
                    break;
                }
                for chunk in items.chunks(25) {
                    let writes: Vec<WriteRequest> = chunk
                        .iter()
                        .filter_map(|item: &HashMap<String, AttributeValue>| {
                            let key = match key_kind {
                                "sub" => {
                                    let id = item.get("subscription_id")?;
                                    HashMap::from([("subscription_id".to_string(), id.clone())])
                                }
                                _ => {
                                    let pk = item.get("pk")?;
                                    let sk = item.get("sk")?;
                                    HashMap::from([
                                        ("pk".to_string(), pk.clone()),
                                        ("sk".to_string(), sk.clone()),
                                    ])
                                }
                            };
                            let dr = DeleteRequest::builder().set_key(Some(key)).build().ok()?;
                            Some(WriteRequest::builder().delete_request(dr).build())
                        })
                        .collect();
                    if writes.is_empty() {
                        continue;
                    }
                    self.client
                        .batch_write_item()
                        .request_items(table.as_str(), writes)
                        .send()
                        .await
                        .map_err(|e| AdapterError::Seed(e.to_string()))?;
                }
            }
        }
        Ok(())
    }

    async fn reconcile_subscriptions(&self) -> Result<(), AdapterError> {
        let resp = self
            .client
            .scan()
            .table_name(&self.subscription_table)
            .consistent_read(true)
            .send()
            .await
            .map_err(|e| AdapterError::Revoke(e.to_string()))?;
        for item in resp.items() {
            let is_active = item
                .get("state")
                .and_then(|v| v.as_s().ok())
                .is_some_and(|s| s.as_str() == "active");
            if !is_active {
                continue;
            }
            let Some(consumer) = item.get("consumer").and_then(|v| v.as_s().ok()).map(String::from) else { continue; };
            let Some(producer) = item.get("producer_domain").and_then(|v| v.as_s().ok()).map(String::from) else { continue; };
            let Some(schema) = item.get("schema").and_then(|v| v.as_s().ok()).map(String::from) else { continue; };
            let Some(sub_id) = item.get("subscription_id").and_then(|v| v.as_s().ok()).map(String::from) else { continue; };
            if !self.has_coverage(&consumer, &producer, &schema).await? {
                self.client
                    .update_item()
                    .table_name(&self.subscription_table)
                    .key("subscription_id", AttributeValue::S(sub_id))
                    .update_expression("SET #st = :disabled")
                    .expression_attribute_names("#st", "state")
                    .expression_attribute_values(":disabled", AttributeValue::S("disabled".to_string()))
                    .send()
                    .await
                    .map_err(|e| AdapterError::Revoke(e.to_string()))?;
            }
        }
        Ok(())
    }
}

impl crate::adapter::GrantAdapter for DynamoGrantAdapter {
    fn seed(
        &self,
        grants: &[GrantSeed],
        subscriptions: &[SubscriptionSeed],
    ) -> impl Future<Output = Result<(), AdapterError>> + Send {
        let grants = grants.to_vec();
        let subscriptions = subscriptions.to_vec();
        async move {
            self.clear_tables().await?;
            for grant in &grants {
                if grant.item_type == "RoleBinding" {
                    let Some(principal) = grant.principal.as_ref() else { continue };
                    let pk = match principal.kind.as_str() {
                        "cognito-group" => format!(
                            "PRINCIPAL#cognito-group#{}#{}",
                            self.trusted_issuer,
                            principal.group.as_deref().unwrap_or_default()
                        ),
                        "aws-account" => format!(
                            "PRINCIPAL#aws-account#{}",
                            principal.account_id.as_deref().unwrap_or_default()
                        ),
                        _ => continue,
                    };
                    let role = grant.role.clone().unwrap_or_default();
                    let domain = grant.domain.clone().unwrap_or_default();
                    let sk = binding_sort_key(&role, &domain);
                    self.client
                        .put_item()
                        .table_name(&self.grant_table)
                        .item("pk", AttributeValue::S(pk))
                        .item("sk", AttributeValue::S(sk))
                        .item("grant_id", AttributeValue::S(grant.grant_id.clone()))
                        .item("role", AttributeValue::S(role))
                        .item("domain", AttributeValue::S(domain))
                        .item("state", AttributeValue::S(grant.state.clone()))
                        .send()
                        .await
                        .map_err(|e| AdapterError::Seed(e.to_string()))?;
                } else if grant.item_type == "Entitlement" {
                    let consumer = grant.consumer.clone().unwrap_or_default();
                    let producer = grant.producer_domain.clone().unwrap_or_default();
                    let pk = format!("CONSUMER#{consumer}");
                    let sk = entitlement_sort_key(&producer, grant.schema.as_deref());
                    let mut put = self
                        .client
                        .put_item()
                        .table_name(&self.grant_table)
                        .item("pk", AttributeValue::S(pk))
                        .item("sk", AttributeValue::S(sk))
                        .item("grant_id", AttributeValue::S(grant.grant_id.clone()))
                        .item("consumer", AttributeValue::S(consumer))
                        .item("producer_domain", AttributeValue::S(producer))
                        .item("scope", AttributeValue::S(grant.scope.clone().unwrap_or_default()))
                        .item("state", AttributeValue::S(grant.state.clone()));
                    if let Some(schema) = grant.schema.as_ref() {
                        put = put.item("schema", AttributeValue::S(schema.clone()));
                    }
                    put.send()
                        .await
                        .map_err(|e| AdapterError::Seed(e.to_string()))?;
                }
            }
            for sub in &subscriptions {
                self.client
                    .put_item()
                    .table_name(&self.subscription_table)
                    .item("subscription_id", AttributeValue::S(sub.subscription_id.clone()))
                    .item("consumer", AttributeValue::S(sub.consumer.clone()))
                    .item("producer_domain", AttributeValue::S(sub.producer_domain.clone()))
                    .item("schema", AttributeValue::S(sub.schema.clone()))
                    .item("state", AttributeValue::S(sub.state.clone()))
                    .send()
                    .await
                    .map_err(|e| AdapterError::Seed(e.to_string()))?;
            }
            Ok(())
        }
    }

    fn decide(
        &self,
        subject: &AuthorizationSubject,
        action: &Action,
        resource: &Resource,
        context: &AuthContext,
    ) -> impl Future<Output = Result<Decision, AdapterError>> + Send {
        let subject = subject.clone();
        let action = action.clone();
        let resource = resource.clone();
        let context = context.clone();
        async move {
            if context.missing_field.is_some() {
                return Ok(Decision::Deny { reason: DenyReason::IncompletePrincipalContext });
            }
            if context.tampered == Some(true) {
                return Ok(Decision::Deny { reason: DenyReason::TamperedPrincipalContext });
            }
            if let Some(replay_of) = context.replay_of.as_ref() {
                if self.seen_commands.lock().expect("poisoned").contains(replay_of) {
                    return Ok(Decision::Deny { reason: DenyReason::DuplicateCommand });
                }
            }
            if !context.command_id.is_empty() {
                self.seen_commands.lock().expect("poisoned").insert(context.command_id.clone());
            }
            if let Some(reason) = self.check_issuer(&subject, &context) {
                return Ok(Decision::Deny { reason });
            }
            let consumer = subject
                .consumer_domain
                .as_deref()
                .or(context.acting_as.as_deref())
                .unwrap_or_default();
            Ok(match action {
                Action::PlatformAdmin => match context.global_role.as_deref() {
                    Some("platform-admin") => Decision::Allow,
                    Some("platform-auditor") => Decision::Deny { reason: DenyReason::AuditorRoleNotAuthorizedForMutation },
                    _ => Decision::Deny { reason: DenyReason::Unknown },
                },
                Action::PlatformRead | Action::DomainRegister => match context.global_role.as_deref() {
                    Some("platform-admin") => Decision::Allow,
                    Some("platform-auditor") if matches!(action, Action::PlatformRead) => Decision::Allow,
                    _ => Decision::Deny { reason: DenyReason::Unknown },
                },
                Action::SchemaPublish => {
                    let Resource::Schema { producer_domain, .. } = resource else {
                        return Ok(Decision::Deny { reason: DenyReason::Unknown });
                    };
                    if self.get_binding(&subject, "publisher", &producer_domain).await? {
                        Decision::Allow
                    } else {
                        Decision::Deny { reason: DenyReason::PublisherNotBoundToDomain }
                    }
                }
                Action::SubscriptionAdmin => {
                    let target = match &resource {
                        Resource::Subscription { consumer_domain, .. } => consumer_domain.as_str(),
                        _ => consumer,
                    };
                    if self.get_binding(&subject, "subscriber", target).await? {
                        Decision::Allow
                    } else {
                        Decision::Deny { reason: DenyReason::SubscriberNotBoundToConsumerDomain }
                    }
                }
                Action::SchemaSubscribe => {
                    let Resource::Schema { producer_domain, schema } = resource else {
                        return Ok(Decision::Deny { reason: DenyReason::Unknown });
                    };
                    if !self.get_binding(&subject, "subscriber", consumer).await? {
                        return Ok(Decision::Deny { reason: DenyReason::SubscriberNotBoundToConsumerDomain });
                    }
                    if self.has_coverage(consumer, &producer_domain, &schema).await? {
                        Decision::Allow
                    } else {
                        Decision::Deny { reason: DenyReason::NoApplicableEntitlement }
                    }
                }
                Action::SchemaDiscover => {
                    let Resource::Schema { producer_domain, schema } = resource else {
                        return Ok(Decision::Deny { reason: DenyReason::Unknown });
                    };
                    if !self.schema_exists(&producer_domain, &schema).await? {
                        return Ok(Decision::DenyAsUnknown);
                    }
                    if !self.get_binding(&subject, "subscriber", consumer).await? {
                        return Ok(Decision::DenyAsUnknown);
                    }
                    if self.has_coverage(consumer, &producer_domain, &schema).await? {
                        Decision::Allow
                    } else {
                        Decision::DenyAsUnknown
                    }
                }
            })
        }
    }

    fn revoke(&self, grant_id: &str) -> impl Future<Output = Result<(), AdapterError>> + Send {
        let grant_id = grant_id.to_string();
        async move {
            let resp = self
                .client
                .query()
                .table_name(&self.grant_table)
                .index_name("grant_id-index")
                .key_condition_expression("grant_id = :gid")
                .expression_attribute_values(":gid", AttributeValue::S(grant_id))
                .send()
                .await
                .map_err(|e| AdapterError::Revoke(e.to_string()))?;
            for item in resp.items() {
                if let (Some(pk), Some(sk)) = (
                    item.get("pk").and_then(|v| v.as_s().ok()),
                    item.get("sk").and_then(|v| v.as_s().ok()),
                ) {
                    self.client
                        .update_item()
                        .table_name(&self.grant_table)
                        .key("pk", AttributeValue::S(pk.to_string()))
                        .key("sk", AttributeValue::S(sk.to_string()))
                        .update_expression("SET #st = :revoked")
                        .expression_attribute_names("#st", "state")
                        .expression_attribute_values(":revoked", AttributeValue::S("revoked".to_string()))
                        .send()
                        .await
                        .map_err(|e| AdapterError::Revoke(e.to_string()))?;
                }
            }
            self.reconcile_subscriptions().await?;
            Ok(())
        }
    }

    fn register_schema(
        &self,
        producer_domain: &str,
        schema: &str,
    ) -> impl Future<Output = Result<(), AdapterError>> + Send {
        let producer_domain = producer_domain.to_string();
        let schema = schema.to_string();
        async move {
            let pk = format!("SCHEMA#{producer_domain}");
            let sk = format!("SCHEMA#{schema}");
            self.client
                .put_item()
                .table_name(&self.grant_table)
                .item("pk", AttributeValue::S(pk))
                .item("sk", AttributeValue::S(sk))
                .item("producer_domain", AttributeValue::S(producer_domain))
                .item("schema", AttributeValue::S(schema))
                .send()
                .await
                .map_err(|e| AdapterError::SchemaRegistration(e.to_string()))?;
            Ok(())
        }
    }

    fn subscription_state(
        &self,
        subscription_id: &str,
    ) -> impl Future<Output = Result<String, AdapterError>> + Send {
        let subscription_id = subscription_id.to_string();
        async move {
            let resp = self
                .client
                .get_item()
                .table_name(&self.subscription_table)
                .key("subscription_id", AttributeValue::S(subscription_id))
                .consistent_read(true)
                .send()
                .await
                .map_err(|e| AdapterError::SubscriptionState(e.to_string()))?;
            Ok(resp
                .item
                .and_then(|item| item.get("state").and_then(|v| v.as_s().ok()).map(String::from))
                .unwrap_or_else(|| "unknown".to_string()))
        }
    }

    fn discover_list(
        &self,
        subject: &AuthorizationSubject,
        producer_domain: &str,
        context: &AuthContext,
    ) -> impl Future<Output = Result<Vec<String>, AdapterError>> + Send {
        let subject = subject.clone();
        let producer_domain = producer_domain.to_string();
        let context = context.clone();
        async move {
            let consumer = subject
                .consumer_domain
                .as_deref()
                .or(context.acting_as.as_deref())
                .unwrap_or_default();
            if !self.get_binding(&subject, "subscriber", consumer).await? {
                return Ok(Vec::new());
            }
            let pk = format!("SCHEMA#{producer_domain}");
            let resp = self
                .client
                .query()
                .table_name(&self.grant_table)
                .key_condition_expression("pk = :pk")
                .expression_attribute_values(":pk", AttributeValue::S(pk))
                .consistent_read(true)
                .send()
                .await
                .map_err(|e| AdapterError::DiscoverList(e.to_string()))?;
            let schemas: Vec<String> = resp
                .items()
                .iter()
                .filter_map(|item| {
                    item.get("schema")
                        .and_then(|v| v.as_s().ok())
                        .map(|s| s.as_str().to_string())
                })
                .collect();
            let mut visible = Vec::new();
            for schema in &schemas {
                if self.has_coverage(consumer, &producer_domain, schema).await? {
                    visible.push(schema.clone());
                }
            }
            visible.sort();
            Ok(visible)
        }
    }
}
