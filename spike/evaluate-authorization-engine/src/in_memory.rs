use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::Mutex;

use crate::adapter::AdapterError;
use crate::contract::{
    Action, AuthContext, AuthorizationSubject, Decision, DenyReason, PrincipalType, Resource,
};
use crate::manifest::{GrantSeed, SubscriptionSeed};
use crate::store::{
    binding_sort_key, principal_key, AuditRecord, BindingItem, EntitlementItem, GrantState,
    SubscriptionItem,
};

/// In-memory grant adapter mirroring the representative single-table grant model
/// access patterns (key-condition lookups, no scans). Proves the model and the
/// full scenario suite locally before the DynamoDB-backed adapter is exercised.
pub struct InMemoryGrantAdapter {
    state: Mutex<AdapterState>,
}

struct AdapterState {
    trusted_issuer: String,
    bindings: HashMap<String, BindingItem>,
    entitlements: HashMap<String, Vec<EntitlementItem>>,
    subscriptions: HashMap<String, SubscriptionItem>,
    grant_state: HashMap<String, GrantState>,
    schemas: HashSet<(String, String)>,
    seen_commands: HashSet<String>,
    audit: Vec<AuditRecord>,
}

impl InMemoryGrantAdapter {
    pub fn with_trusted_issuer(issuer: impl Into<String>) -> Self {
        Self {
            state: Mutex::new(AdapterState {
                trusted_issuer: issuer.into(),
                bindings: HashMap::new(),
                entitlements: HashMap::new(),
                subscriptions: HashMap::new(),
                grant_state: HashMap::new(),
                schemas: HashSet::new(),
                seen_commands: HashSet::new(),
                audit: Vec::new(),
            }),
        }
    }
}

impl crate::adapter::GrantAdapter for InMemoryGrantAdapter {
    fn seed(
        &self,
        grants: &[GrantSeed],
        subscriptions: &[SubscriptionSeed],
    ) -> impl Future<Output = Result<(), AdapterError>> + Send {
        let grants = grants.to_vec();
        let subscriptions = subscriptions.to_vec();
        async move {
            let mut state = self.state.lock().expect("adapter state poisoned");
            let trusted_issuer = state.trusted_issuer.clone();
            state.reset();
            for grant in &grants {
                state
                    .grant_state
                    .insert(grant.grant_id.clone(), GrantState::Active);
                if grant.item_type == "RoleBinding" {
                    seed_binding(&mut state, grant, &trusted_issuer);
                } else if grant.item_type == "Entitlement" {
                    seed_entitlement(&mut state, grant);
                }
            }
            for sub in &subscriptions {
                state.subscriptions.insert(
                    sub.subscription_id.clone(),
                    SubscriptionItem {
                        subscription_id: sub.subscription_id.clone(),
                        consumer: sub.consumer.clone(),
                        producer_domain: sub.producer_domain.clone(),
                        schema: sub.schema.clone(),
                        state: sub.state.clone(),
                    },
                );
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
            let mut state = self.state.lock().expect("adapter state poisoned");
            Ok(state.decide(&subject, &action, &resource, &context))
        }
    }

    fn revoke(&self, grant_id: &str) -> impl Future<Output = Result<(), AdapterError>> + Send {
        let grant_id = grant_id.to_string();
        async move {
            let mut state = self.state.lock().expect("adapter state poisoned");
            state.revoke(&grant_id);
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
            let mut state = self.state.lock().expect("adapter state poisoned");
            state.schemas.insert((producer_domain, schema));
            Ok(())
        }
    }

    fn subscription_state(
        &self,
        subscription_id: &str,
    ) -> impl Future<Output = Result<String, AdapterError>> + Send {
        let subscription_id = subscription_id.to_string();
        async move {
            let state = self.state.lock().expect("adapter state poisoned");
            Ok(state
                .subscriptions
                .get(&subscription_id)
                .map(|s| s.state.clone())
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
            let state = self.state.lock().expect("adapter state poisoned");
            Ok(state.discover_list(&subject, &producer_domain, &context))
        }
    }
}

fn seed_binding(state: &mut AdapterState, grant: &GrantSeed, trusted_issuer: &str) {
    let principal = match grant.principal.as_ref() {
        Some(p) => p,
        None => return,
    };
    let pk = match principal.kind.as_str() {
        "cognito-group" => format!(
            "PRINCIPAL#cognito-group#{trusted_issuer}#{}",
            principal.group.as_deref().unwrap_or_default()
        ),
        "aws-account" => format!(
            "PRINCIPAL#aws-account#{}",
            principal.account_id.as_deref().unwrap_or_default()
        ),
        _ => return,
    };
    let role = grant.role.clone().unwrap_or_default();
    let domain = grant.domain.clone().unwrap_or_default();
    let key = format!("{pk}#{}", binding_sort_key(&role, &domain));
    state.bindings.insert(
        key,
        BindingItem {
            grant_id: grant.grant_id.clone(),
            role,
            domain,
            state: GrantState::Active,
        },
    );
}

fn seed_entitlement(state: &mut AdapterState, grant: &GrantSeed) {
    let consumer = grant.consumer.clone().unwrap_or_default();
    let producer_domain = grant.producer_domain.clone().unwrap_or_default();
    let schema = grant.schema.clone();
    let scope = grant.scope.clone().unwrap_or_default();
    state
        .entitlements
        .entry(consumer.clone())
        .or_default()
        .push(EntitlementItem {
            grant_id: grant.grant_id.clone(),
            consumer,
            producer_domain,
            schema,
            scope,
            state: GrantState::Active,
        });
}

impl AdapterState {
    fn reset(&mut self) {
        self.bindings.clear();
        self.entitlements.clear();
        self.subscriptions.clear();
        self.grant_state.clear();
        self.schemas.clear();
        self.seen_commands.clear();
        self.audit.clear();
    }

    fn validate_context(
        &mut self,
        subject: &AuthorizationSubject,
        context: &AuthContext,
    ) -> Option<Decision> {
        if context.missing_field.is_some() {
            return Some(Decision::Deny {
                reason: DenyReason::IncompletePrincipalContext,
            });
        }
        if context.tampered == Some(true) {
            return Some(Decision::Deny {
                reason: DenyReason::TamperedPrincipalContext,
            });
        }
        if let Some(replay_of) = context.replay_of.as_ref() {
            if self.seen_commands.contains(replay_of) {
                return Some(Decision::Deny {
                    reason: DenyReason::DuplicateCommand,
                });
            }
        }
        if !context.command_id.is_empty() {
            self.seen_commands.insert(context.command_id.clone());
        }
        self.check_issuer(subject, context)
            .map(|reason| Decision::Deny { reason })
    }

    fn decide_schema_action(
        &self,
        action: &Action,
        subject: &AuthorizationSubject,
        resource: &Resource,
        context: &AuthContext,
    ) -> Decision {
        let Resource::Schema {
            producer_domain,
            schema,
        } = resource
        else {
            return Decision::Deny {
                reason: DenyReason::Unknown,
            };
        };
        let consumer = resolve_consumer(subject, context);
        match action {
            Action::SchemaSubscribe => {
                if !self.has_active_binding(subject, "subscriber", consumer) {
                    return Decision::Deny {
                        reason: DenyReason::SubscriberNotBoundToConsumerDomain,
                    };
                }
                if self.has_coverage(consumer, producer_domain, schema) {
                    Decision::Allow
                } else {
                    Decision::Deny {
                        reason: DenyReason::NoApplicableEntitlement,
                    }
                }
            }
            Action::SchemaDiscover => {
                if !self.has_active_binding(subject, "subscriber", consumer) {
                    return Decision::DenyAsUnknown;
                }
                if !self
                    .schemas
                    .contains(&(producer_domain.to_string(), schema.to_string()))
                {
                    return Decision::DenyAsUnknown;
                }
                if self.has_coverage(consumer, producer_domain, schema) {
                    Decision::Allow
                } else {
                    Decision::DenyAsUnknown
                }
            }
            _ => Decision::Deny {
                reason: DenyReason::Unknown,
            },
        }
    }

    fn decide(
        &mut self,
        subject: &AuthorizationSubject,
        action: &Action,
        resource: &Resource,
        context: &AuthContext,
    ) -> Decision {
        if let Some(early_deny) = self.validate_context(subject, context) {
            return early_deny;
        }
        match action {
            Action::PlatformAdmin => match context.global_role.as_deref() {
                Some("platform-admin") => Decision::Allow,
                Some("platform-auditor") => Decision::Deny {
                    reason: DenyReason::AuditorRoleNotAuthorizedForMutation,
                },
                _ => Decision::Deny {
                    reason: DenyReason::Unknown,
                },
            },
            Action::PlatformRead | Action::DomainRegister => match context.global_role.as_deref() {
                Some("platform-admin") => Decision::Allow,
                Some("platform-auditor") if matches!(action, Action::PlatformRead) => {
                    Decision::Allow
                }
                _ => Decision::Deny {
                    reason: DenyReason::Unknown,
                },
            },
            Action::SchemaPublish => {
                let Resource::Schema {
                    producer_domain, ..
                } = resource
                else {
                    return Decision::Deny {
                        reason: DenyReason::Unknown,
                    };
                };
                if self.has_active_binding(subject, "publisher", producer_domain) {
                    Decision::Allow
                } else {
                    Decision::Deny {
                        reason: DenyReason::PublisherNotBoundToDomain,
                    }
                }
            }
            Action::SubscriptionAdmin => {
                let consumer = match resource {
                    Resource::Subscription {
                        consumer_domain, ..
                    } => consumer_domain.as_str(),
                    _ => context.acting_as.as_deref().unwrap_or_default(),
                };
                if self.has_active_binding(subject, "subscriber", consumer) {
                    Decision::Allow
                } else {
                    Decision::Deny {
                        reason: DenyReason::SubscriberNotBoundToConsumerDomain,
                    }
                }
            }
            Action::SchemaSubscribe | Action::SchemaDiscover => {
                self.decide_schema_action(action, subject, resource, context)
            }
        }
    }

    fn check_issuer(
        &self,
        subject: &AuthorizationSubject,
        context: &AuthContext,
    ) -> Option<DenyReason> {
        if subject.kind != PrincipalType::CognitoGroup {
            return None;
        }
        let trusted = context
            .trusted_issuer
            .as_deref()
            .unwrap_or(&self.trusted_issuer);
        if subject.issuer.as_deref() != Some(trusted) {
            return Some(DenyReason::UntrustedIssuer);
        }
        None
    }

    fn has_active_binding(&self, subject: &AuthorizationSubject, role: &str, domain: &str) -> bool {
        let pk = principal_key(subject);
        let key = format!("{pk}#{}", binding_sort_key(role, domain));
        self.bindings
            .get(&key)
            .map(|b| b.state == GrantState::Active)
            .unwrap_or(false)
    }

    fn has_coverage(&self, consumer: &str, producer: &str, schema: &str) -> bool {
        self.entitlements
            .get(consumer)
            .into_iter()
            .flatten()
            .any(|e| {
                e.state == GrantState::Active
                    && e.producer_domain == producer
                    && (e.schema.is_none() || e.schema.as_deref() == Some(schema))
            })
    }

    fn discover_list(
        &self,
        subject: &AuthorizationSubject,
        producer: &str,
        context: &AuthContext,
    ) -> Vec<String> {
        let consumer = resolve_consumer(subject, context);
        if !self.has_active_binding(subject, "subscriber", consumer) {
            return Vec::new();
        }
        let mut visible: Vec<String> = self
            .schemas
            .iter()
            .filter(|(p, _)| p == producer)
            .filter(|(p, s)| self.has_coverage(consumer, p, s))
            .map(|(_, s)| s.clone())
            .collect();
        visible.sort();
        visible
    }

    fn revoke(&mut self, grant_id: &str) {
        self.grant_state
            .insert(grant_id.to_string(), GrantState::Revoked);
        for binding in self.bindings.values_mut() {
            if binding.grant_id == grant_id {
                binding.state = GrantState::Revoked;
            }
        }
        for entitlements in self.entitlements.values_mut() {
            for entitlement in entitlements.iter_mut() {
                if entitlement.grant_id == grant_id {
                    entitlement.state = GrantState::Revoked;
                }
            }
        }
        self.audit.push(AuditRecord {
            grant_id: grant_id.to_string(),
            action: "revoke".to_string(),
            timestamp_ms: 0,
        });
        self.reconcile_subscriptions();
    }

    fn reconcile_subscriptions(&mut self) {
        let subscriptions: Vec<SubscriptionItem> = self.subscriptions.values().cloned().collect();
        for subscription in subscriptions {
            if subscription.state == "active"
                && !self.has_coverage(
                    &subscription.consumer,
                    &subscription.producer_domain,
                    &subscription.schema,
                )
            {
                if let Some(stored) = self.subscriptions.get_mut(&subscription.subscription_id) {
                    stored.state = "disabled".to_string();
                }
            }
        }
    }
}

fn resolve_consumer<'a>(subject: &'a AuthorizationSubject, context: &'a AuthContext) -> &'a str {
    subject
        .consumer_domain
        .as_deref()
        .or(context.acting_as.as_deref())
        .unwrap_or_default()
}
