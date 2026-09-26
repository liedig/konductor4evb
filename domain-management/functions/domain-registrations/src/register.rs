use anyhow::{Context, Result};
use aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsError;
use aws_sdk_dynamodb::types::{
    AttributeValue, Put, ReturnValuesOnConditionCheckFailure, TransactWriteItem,
};
use aws_sdk_sfn::operation::start_execution::StartExecutionError;
use serde_json::json;
use std::collections::HashMap;
use std::env;

use konductor_common::dynamodb::{
    domain_count_increment, get_s, s, string_map_to_attr, DOMAIN_COUNT_CAP,
};
use konductor_common::error::ControlPlaneError;
use konductor_common::models::{DomainStatus, RegisterDomainRequest};
use konductor_common::principal::TrustedPrincipal;

pub async fn handle(
    clients: &crate::handler::Clients,
    request: &RegisterDomainRequest,
    principal: &TrustedPrincipal,
    request_id: &str,
) -> Result<()> {
    let crate::handler::Clients { ddb, eb, sfn, .. } = clients;
    let domains_table = env::var("DOMAINS_TABLE").context("DOMAINS_TABLE not set")?;
    let state_machine_arn =
        env::var("APPROVAL_STATE_MACHINE_ARN").context("APPROVAL_STATE_MACHINE_ARN not set")?;
    let cp_bus = env::var("KONDUKTOR_EVENT_BUS").context("KONDUKTOR_EVENT_BUS not set")?;
    let org = env::var("ORGANIZATION").context("ORGANIZATION not set")?;
    let environments_raw = env::var("ENVIRONMENTS").context("ENVIRONMENTS not set")?;
    let environments = parse_environments(&environments_raw).map_err(|e| {
        ControlPlaneError::ValidationException {
            message: format!("invalid ENVIRONMENTS value: {e}"),
            reason: konductor_common::error::ValidationExceptionReason::FieldValidationFailed,
            field_list: None,
        }
    })?;

    if reject_invalid_request(eb, &cp_bus, request, principal, &environments).await? {
        return Ok(());
    }

    if let Some(justification) = run_preflight(clients, &org, request, &environments).await? {
        tracing::warn!(domain = %request.domain_name, %justification, "registration preflight rejected");
        return reject_for_collision(
            clients,
            CollisionRejection {
                table: &domains_table,
                org: &org,
                bus: &cp_bus,
                request,
                request_id,
                environments: &environments,
                justification: &justification,
            },
        )
        .await;
    }

    let item = registration_item(&org, request, request_id, &environments);
    let created_at = get_s(&item, "createdAt").context("registration item missing createdAt")?;

    let outcome = write_domain_row(ddb, &domains_table, &item, request_id).await?;
    let Some(created_at) = resolve_write_outcome(outcome, eb, &cp_bus, request, created_at).await?
    else {
        return Ok(());
    };

    start_approval_workflow(sfn, &state_machine_arn, request, request_id, &environments).await?;

    emit_succeeded(eb, &cp_bus, request, request_id, &environments, &created_at).await
}

/// Maps the write outcome to the registration flow: Ok(Some(created_at))
/// proceeds to the approval workflow; Ok(None) has already emitted the
/// deterministic failure and the caller returns.
async fn resolve_write_outcome(
    outcome: RowWriteOutcome,
    eb: &aws_sdk_eventbridge::Client,
    cp_bus: &str,
    request: &RegisterDomainRequest,
    created_at: String,
) -> Result<Option<String>> {
    match outcome {
        RowWriteOutcome::Created | RowWriteOutcome::OverwroteRejected => Ok(Some(created_at)),
        RowWriteOutcome::IdempotentRetry {
            created_at: original,
        } => Ok(Some(original)),
        RowWriteOutcome::Duplicate => {
            let reason = format!("domain '{}' already exists", request.domain_name);
            emit_failed(eb, cp_bus, request, &reason).await?;
            Ok(None)
        }
        RowWriteOutcome::LimitExceeded => {
            let limit = ControlPlaneError::DomainLimitExceeded {
                cap: DOMAIN_COUNT_CAP as u32,
            };
            tracing::warn!(domain = %request.domain_name, "registration rejected: domain cap reached");
            emit_failed(eb, cp_bus, request, &limit.to_string()).await?;
            Ok(None)
        }
    }
}

/// Validation that runs before the record exists: the governed custom tag is
/// legal and no platform-owned resource names are taken. Returns the rejection
/// justification when the request must not proceed.
async fn run_preflight(
    clients: &crate::handler::Clients,
    org: &str,
    request: &RegisterDomainRequest,
    environments: &[String],
) -> Result<Option<String>> {
    if let Some(cc) = &request.cost_center {
        if let Err(reason) = konductor_common::tags::validate_cost_center(cc) {
            return Ok(Some(reason));
        }
    }
    let collisions = find_collisions(
        &clients.eb,
        &clients.ssm,
        org,
        &request.domain_name,
        environments,
    )
    .await?;
    if collisions.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "resource name collision: {}",
        collisions.join(", ")
    )))
}

/// Pre-registration guard: the platform-owned resource names a domain will
/// occupy must be free, or the provisioning saga fails at its first step —
/// after a human already approved. Platform account only; check errors are
/// not proof of absence, so they fail the command for queue redrive.
async fn find_collisions(
    eb: &aws_sdk_eventbridge::Client,
    ssm: &aws_sdk_ssm::Client,
    org: &str,
    domain: &str,
    environments: &[String],
) -> Result<Vec<String>> {
    let resolver = konductor_common::namespace::NamespaceResolver::default_pattern();
    let mut collisions = Vec::new();

    for env_name in environments {
        let bus_name = resolver.event_bus_name(env_name, org, domain);
        match eb.describe_event_bus().name(&bus_name).send().await {
            Ok(_) => collisions.push(format!("event bus {bus_name}")),
            Err(e) => {
                let service_err = e.into_service_error();
                if !matches!(
                    service_err,
                    aws_sdk_eventbridge::operation::describe_event_bus::DescribeEventBusError::ResourceNotFoundException(_)
                ) {
                    return Err(anyhow::anyhow!(
                        "collision check failed for event bus {bus_name}: {service_err}"
                    ));
                }
            }
        }
    }

    let parameter = resolver.namespace_parameter_path(domain);
    match ssm.get_parameter().name(&parameter).send().await {
        Ok(_) => collisions.push(format!("SSM parameter {parameter}")),
        Err(e) => {
            let service_err = e.into_service_error();
            if !matches!(
                service_err,
                aws_sdk_ssm::operation::get_parameter::GetParameterError::ParameterNotFound(_)
            ) {
                return Err(anyhow::anyhow!(
                    "collision check failed for {parameter}: {service_err}"
                ));
            }
        }
    }

    Ok(collisions)
}

/// A colliding request becomes a system rejection: visible to the requester,
/// non-blocking for the name (the conditional put admits REJECTED records),
/// and never reaches the approval workflow.
struct CollisionRejection<'a> {
    table: &'a str,
    org: &'a str,
    bus: &'a str,
    request: &'a RegisterDomainRequest,
    request_id: &'a str,
    environments: &'a [String],
    justification: &'a str,
}

async fn reject_for_collision(
    clients: &crate::handler::Clients,
    rejection: CollisionRejection<'_>,
) -> Result<()> {
    let crate::handler::Clients { ddb, eb, .. } = clients;
    let CollisionRejection {
        table: domains_table,
        org,
        bus: cp_bus,
        request,
        request_id,
        environments,
        justification,
    } = rejection;
    let item = collision_item(org, request, request_id, environments, justification);

    match write_domain_row(ddb, domains_table, &item, request_id).await? {
        RowWriteOutcome::Created | RowWriteOutcome::OverwroteRejected => {
            emit_failed(eb, cp_bus, request, justification).await
        }
        RowWriteOutcome::Duplicate | RowWriteOutcome::IdempotentRetry { .. } => {
            // A live (non-rejected) record won the race — the request is a
            // duplicate, not a collision rejection.
            let reason = format!("domain '{}' already exists", request.domain_name);
            emit_failed(eb, cp_bus, request, &reason).await
        }
        RowWriteOutcome::LimitExceeded => {
            let limit = ControlPlaneError::DomainLimitExceeded {
                cap: DOMAIN_COUNT_CAP as u32,
            };
            tracing::warn!(domain = %request.domain_name, "collision rejection suppressed: domain cap reached");
            emit_failed(eb, cp_bus, request, &limit.to_string()).await
        }
    }
}

/// OAuth callers register on behalf of exactly one of their team groups,
/// which becomes the domain's owner at approval. Global platform groups are
/// never carried as team groups, so membership alone enforces "platform
/// teams cannot own domains". IAM automation keeps its trust path and may
/// omit the team; approval then creates no owner binding.
fn authorize_registration(
    principal: &TrustedPrincipal,
    request: &RegisterDomainRequest,
) -> Result<(), String> {
    match principal {
        TrustedPrincipal::Iam(_) => Ok(()),
        TrustedPrincipal::OAuth(p) => {
            let Some(team) = request.owning_team.as_deref() else {
                return Err("unauthorized: OAuth registration requires owningTeam naming one of the caller's team groups".to_string());
            };
            if p.team_groups.iter().any(|g| g == team) {
                Ok(())
            } else {
                Err(format!(
                    "unauthorized: owningTeam '{team}' is not one of the caller's team groups"
                ))
            }
        }
    }
}

/// The caller-supplied overrides normalized for storage and resolution: an
/// empty map is indistinguishable from an absent one, so neither is stored.
fn requested_overrides(request: &RegisterDomainRequest) -> Option<&HashMap<String, String>> {
    request
        .environment_accounts
        .as_ref()
        .filter(|m| !m.is_empty())
}

/// The rejection reason when the sparse override map names an environment
/// outside the platform list or a malformed account ID; None when valid or
/// absent.
fn invalid_overrides_reason(
    request: &RegisterDomainRequest,
    environments: &[String],
) -> Option<String> {
    let overrides = requested_overrides(request)?;
    konductor_common::environment_accounts::validate_environment_accounts(overrides, environments)
        .err()
}

/// The deterministic request rejections — authorization, domain-name
/// syntax, and the environmentAccounts contract — emit RegisterDomainFailed
/// and ack the message. Returns true when the request was rejected.
async fn reject_invalid_request(
    eb: &aws_sdk_eventbridge::Client,
    cp_bus: &str,
    request: &RegisterDomainRequest,
    principal: &TrustedPrincipal,
    environments: &[String],
) -> Result<bool> {
    if let Some(reason) = authorize_registration(principal, request).err() {
        tracing::warn!(action = "domains:Register", domain = %request.domain_name, "registration authorization denied");
        emit_failed(eb, cp_bus, request, &reason).await?;
        return Ok(true);
    }

    if let Err(e) = validate_domain_name(&request.domain_name) {
        emit_failed(eb, cp_bus, request, &e.to_string()).await?;
        return Ok(true);
    }

    if let Some(reason) = invalid_overrides_reason(request, environments) {
        tracing::warn!(domain = %request.domain_name, %reason, "registration rejected: invalid environmentAccounts");
        emit_failed(eb, cp_bus, request, &reason).await?;
        return Ok(true);
    }

    Ok(false)
}

fn base_domain_item(
    org: &str,
    request: &RegisterDomainRequest,
    request_id: &str,
    environments: &[String],
) -> HashMap<String, AttributeValue> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut item = HashMap::from([
        ("domainName".to_string(), s(&request.domain_name)),
        ("requestId".to_string(), s(request_id)),
        ("ownerAccountId".to_string(), s(&request.owner_account_id)),
        ("ownerEmail".to_string(), s(&request.owner_email)),
        (
            "secondaryOwnerEmails".to_string(),
            AttributeValue::Ss(request.secondary_owner_emails.clone()),
        ),
        ("createdAt".to_string(), s(&now)),
        ("updatedAt".to_string(), s(&now)),
        ("createdBy".to_string(), s(&request.owner_account_id)),
        (
            "environments".to_string(),
            AttributeValue::L(environments.iter().map(|e| s(e)).collect()),
        ),
    ]);

    if let Some(desc) = &request.description {
        item.insert("description".to_string(), s(desc));
    }
    if let Some(team) = &request.owning_team {
        item.insert("owningTeam".to_string(), s(team));
    }
    if let Some(cc) = request.cost_center.as_deref().filter(|v| !v.is_empty()) {
        item.insert("costCenter".to_string(), s(cc));
    }
    if let Some(overrides) = requested_overrides(request) {
        item.insert(
            "environmentAccounts".to_string(),
            string_map_to_attr(overrides),
        );
    }
    let effective: HashMap<String, String> = konductor_common::tags::effective_tags(
        org,
        &request.domain_name,
        None,
        request.cost_center.as_deref(),
    )
    .into_iter()
    .collect();
    item.insert("tags".to_string(), string_map_to_attr(&effective));
    item
}

fn registration_item(
    org: &str,
    request: &RegisterDomainRequest,
    request_id: &str,
    environments: &[String],
) -> HashMap<String, AttributeValue> {
    let mut item = base_domain_item(org, request, request_id, environments);
    item.insert(
        "status".to_string(),
        s(&DomainStatus::PendingApproval.to_string()),
    );
    item
}

fn collision_item(
    org: &str,
    request: &RegisterDomainRequest,
    request_id: &str,
    environments: &[String],
    justification: &str,
) -> HashMap<String, AttributeValue> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut item = base_domain_item(org, request, request_id, environments);
    item.insert("status".to_string(), s(&DomainStatus::Rejected.to_string()));
    item.insert("decidedBy".to_string(), s("system"));
    item.insert("justification".to_string(), s(justification));
    item.insert("rejectedAt".to_string(), s(&now));
    item
}

/// Result of the two-phase domain-row write.
enum RowWriteOutcome {
    /// New row; the domain-count counter was incremented with it.
    Created,
    /// A rejected row was replaced; the counter already carries its slot.
    OverwroteRejected,
    /// Our own earlier delivery owns the live row; resume from it.
    IdempotentRetry { created_at: String },
    /// A live row with a different request id owns the name.
    Duplicate,
    /// The deployment is at the domain-count cap; nothing was written.
    LimitExceeded,
}

/// Writes the domain row, charging the domain-count counter exactly when a
/// new row is created. DynamoDB conditions cannot reference another item in
/// the same transaction, so "increment only for a new row" is a two-step
/// fallback: the first transaction pairs the row put (name must be absent)
/// with the counter increment (count below the cap), committing or aborting
/// together; when the name is taken, a second transaction overwrites the row
/// only if it is REJECTED, leaving the counter untouched. Cancellation of
/// that second condition means a live record owns the name — our own earlier
/// delivery (idempotent retry) or a duplicate.
async fn write_domain_row(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    item: &HashMap<String, AttributeValue>,
    request_id: &str,
) -> Result<RowWriteOutcome> {
    match try_create_charging_counter(ddb, table, item, request_id).await? {
        Some(outcome) => Ok(outcome),
        None => overwrite_rejected(ddb, table, item, request_id).await,
    }
}

/// First phase: put the row and charge the counter in one transaction.
/// Ok(None) means the name is taken by a REJECTED row — the caller proceeds
/// to the overwrite phase without charging the counter.
async fn try_create_charging_counter(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    item: &HashMap<String, AttributeValue>,
    request_id: &str,
) -> Result<Option<RowWriteOutcome>> {
    let put_new = TransactWriteItem::builder()
        .put(
            Put::builder()
                .table_name(table)
                .set_item(Some(item.clone()))
                .condition_expression("attribute_not_exists(domainName)")
                .return_values_on_condition_check_failure(
                    ReturnValuesOnConditionCheckFailure::AllOld,
                )
                .build()
                .context("building domain-row put")?,
        )
        .build();
    let charge_slot = TransactWriteItem::builder()
        .update(domain_count_increment(table).context("building domain-count increment")?)
        .build();

    let Err(first_err) = ddb
        .transact_write_items()
        .transact_items(put_new)
        .transact_items(charge_slot)
        .send()
        .await
    else {
        return Ok(Some(RowWriteOutcome::Created));
    };

    let service_err = first_err.into_service_error();
    let TransactWriteItemsError::TransactionCanceledException(cancel) = &service_err else {
        return Err(ControlPlaneError::DynamoDb(service_err.to_string()).into());
    };
    let reasons = cancel.cancellation_reasons();
    let condition_failed = |index: usize| {
        reasons
            .get(index)
            .and_then(|r| r.code())
            .is_some_and(|c| c == "ConditionalCheckFailed")
    };

    if !condition_failed(0) {
        // The row put passed its condition, so only the cap can have
        // cancelled deterministically; anything else is transient and goes
        // back to the queue for redrive.
        return if condition_failed(1) {
            Ok(Some(RowWriteOutcome::LimitExceeded))
        } else {
            Err(ControlPlaneError::DynamoDb(service_err.to_string()).into())
        };
    }

    let rejected = DomainStatus::Rejected.to_string();
    let existing = reasons.first().and_then(|r| r.item());
    if existing.and_then(|it| get_s(it, "status")).as_deref() == Some(rejected.as_str()) {
        return Ok(None);
    }
    classify_live_row(existing, request_id).map(Some)
}

/// Second phase: overwrite the REJECTED row without charging the counter.
/// Cancellation means a concurrent decision took the row out of REJECTED —
/// reclassify from the returned row.
async fn overwrite_rejected(
    ddb: &aws_sdk_dynamodb::Client,
    table: &str,
    item: &HashMap<String, AttributeValue>,
    request_id: &str,
) -> Result<RowWriteOutcome> {
    let rejected = DomainStatus::Rejected.to_string();
    let put_over_rejected = TransactWriteItem::builder()
        .put(
            Put::builder()
                .table_name(table)
                .set_item(Some(item.clone()))
                .condition_expression("#status = :rejected")
                .expression_attribute_names("#status", "status")
                .expression_attribute_values(":rejected", s(&rejected))
                .return_values_on_condition_check_failure(
                    ReturnValuesOnConditionCheckFailure::AllOld,
                )
                .build()
                .context("building rejected-overwrite put")?,
        )
        .build();

    match ddb
        .transact_write_items()
        .transact_items(put_over_rejected)
        .send()
        .await
    {
        Ok(_) => Ok(RowWriteOutcome::OverwroteRejected),
        Err(e) => {
            let service_err = e.into_service_error();
            let TransactWriteItemsError::TransactionCanceledException(cancel) = &service_err else {
                return Err(ControlPlaneError::DynamoDb(service_err.to_string()).into());
            };
            let current = cancel.cancellation_reasons().first().and_then(|r| r.item());
            classify_live_row(current, request_id)
        }
    }
}

/// A live row owns the name: our own earlier delivery (idempotent retry) or
/// a genuine duplicate.
fn classify_live_row(
    row: Option<&HashMap<String, AttributeValue>>,
    request_id: &str,
) -> Result<RowWriteOutcome> {
    match row.and_then(|it| get_s(it, "requestId")) {
        Some(id) if id == request_id => {
            let created_at = row
                .and_then(|it| get_s(it, "createdAt"))
                .context("existing row missing createdAt")?;
            Ok(RowWriteOutcome::IdempotentRetry { created_at })
        }
        _ => Ok(RowWriteOutcome::Duplicate),
    }
}

async fn start_approval_workflow(
    sfn: &aws_sdk_sfn::Client,
    state_machine_arn: &str,
    request: &RegisterDomainRequest,
    request_id: &str,
    environments: &[String],
) -> Result<()> {
    let sfn_input = json!({
        "requestId": request_id,
        "domainName": request.domain_name,
        "ownerAccountId": request.owner_account_id,
        "ownerEmail": request.owner_email,
        "secondaryOwnerEmails": request.secondary_owner_emails,
        "description": request.description,
        "costCenter": request.cost_center,
        "environments": environments,
        "environmentAccounts": konductor_common::environment_accounts::effective_accounts(
            requested_overrides(request),
            &request.owner_account_id,
            environments,
        ),
    });

    if let Err(e) = sfn
        .start_execution()
        .state_machine_arn(state_machine_arn)
        .name(format!("approval-{request_id}"))
        .input(sfn_input.to_string())
        .send()
        .await
    {
        let service_err = e.into_service_error();
        if !matches!(service_err, StartExecutionError::ExecutionAlreadyExists(_)) {
            return Err(ControlPlaneError::StepFunctions(service_err.to_string()).into());
        }
    }
    Ok(())
}

async fn emit_succeeded(
    eb: &aws_sdk_eventbridge::Client,
    cp_bus: &str,
    request: &RegisterDomainRequest,
    request_id: &str,
    environments: &[String],
    created_at: &str,
) -> Result<()> {
    let status = DomainStatus::PendingApproval.to_string();
    let result = eb.put_events().entries(
        aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
            .event_bus_name(cp_bus).source("konductor.domain-management").detail_type("RegisterDomainSucceeded")
            .detail(json!({"requestId": request_id, "domainName": request.domain_name, "ownerAccountId": request.owner_account_id, "ownerEmail": request.owner_email, "secondaryOwnerEmails": request.secondary_owner_emails, "status": status, "createdAt": created_at, "environments": environments, "environmentAccounts": konductor_common::environment_accounts::effective_accounts(requested_overrides(request), &request.owner_account_id, environments)}).to_string())
            .build(),
    ).send().await.map_err(|e| ControlPlaneError::EventBridge(e.to_string()))?;

    if result.failed_entry_count > 0 {
        return Err(ControlPlaneError::EventBridge(format!(
            "{} RegisterDomainSucceeded event entries failed to publish",
            result.failed_entry_count
        ))
        .into());
    }
    Ok(())
}

async fn emit_failed(
    eb: &aws_sdk_eventbridge::Client,
    cp_bus: &str,
    request: &RegisterDomainRequest,
    reason: &str,
) -> Result<()> {
    let result = eb
        .put_events()
        .entries(
            aws_sdk_eventbridge::types::PutEventsRequestEntry::builder()
                .event_bus_name(cp_bus)
                .source("konductor.domain-management")
                .detail_type("RegisterDomainFailed")
                .detail(
                    json!({
                        "domainName": request.domain_name,
                        "ownerAccountId": request.owner_account_id,
                        "ownerEmail": request.owner_email,
                        "secondaryOwnerEmails": request.secondary_owner_emails,
                        "reason": reason,
                        "failedAt": chrono::Utc::now().to_rfc3339(),
                    })
                    .to_string(),
                )
                .build(),
        )
        .send()
        .await
        .map_err(|e| ControlPlaneError::EventBridge(e.to_string()))?;

    if result.failed_entry_count > 0 {
        return Err(ControlPlaneError::EventBridge(format!(
            "{} RegisterDomainFailed event entries failed to publish",
            result.failed_entry_count
        ))
        .into());
    }

    Ok(())
}

fn parse_environments(raw: &str) -> Result<Vec<String>, String> {
    let envs: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if envs.is_empty() {
        return Err("must contain at least one entry".to_string());
    }

    for env_name in &envs {
        validate_environment_name(env_name)
            .map_err(|e| format!("invalid entry '{env_name}': {e}"))?;
    }

    Ok(envs)
}

fn validate_environment_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 32 {
        return Err("length must be 1-32 characters".to_string());
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err("length must be 1-32 characters".to_string());
    };
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return Err("must start with a lowercase letter or digit".to_string());
    }
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err("must contain only lowercase letters, digits, and hyphens".to_string());
        }
    }
    Ok(())
}

const RESERVED_DOMAIN_NAMES: &[&str] = &["konductor"];

pub(crate) fn validate_domain_name(name: &str) -> Result<(), ControlPlaneError> {
    if name.len() < 2 || name.len() > 64 {
        return Err(konductor_common::api::validation_error(
            "Domain name must be between 2 and 64 characters",
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(konductor_common::api::validation_error(
            "Domain name must contain only lowercase letters, digits, and hyphens",
        ));
    }
    if name.starts_with('-') || name.ends_with('-') {
        return Err(konductor_common::api::validation_error(
            "Domain name must not start or end with a hyphen",
        ));
    }
    if RESERVED_DOMAIN_NAMES.contains(&name) {
        return Err(konductor_common::api::validation_error(format!(
            "Domain name '{name}' is reserved for platform use"
        )));
    }
    Ok(())
}

/// Preflight mock rules shared by the register and dispatcher test suites.
#[cfg(test)]
pub(crate) mod test_rules {
    use aws_smithy_mocks::{mock, Rule};

    /// Every event bus name is absent.
    pub(crate) fn no_bus_rule() -> Rule {
        mock!(aws_sdk_eventbridge::Client::describe_event_bus).then_error(|| {
            aws_sdk_eventbridge::operation::describe_event_bus::DescribeEventBusError::ResourceNotFoundException(
                aws_sdk_eventbridge::types::error::ResourceNotFoundException::builder()
                    .message("not found")
                    .build(),
            )
        })
    }

    /// The namespace parameter does not exist.
    pub(crate) fn no_param_rule() -> Rule {
        mock!(aws_sdk_ssm::Client::get_parameter).then_error(|| {
            aws_sdk_ssm::operation::get_parameter::GetParameterError::ParameterNotFound(
                aws_sdk_ssm::types::error::ParameterNotFound::builder().build(),
            )
        })
    }

    /// Every event bus name resolves — a full collision set.
    pub(crate) fn existing_bus_rule() -> Rule {
        mock!(aws_sdk_eventbridge::Client::describe_event_bus).then_output(|| {
            aws_sdk_eventbridge::operation::describe_event_bus::DescribeEventBusOutput::builder()
                .name("acme-dev-billing-event-bus")
                .arn("arn:aws:events:us-east-1:123:event-bus/acme-dev-billing-event-bus")
                .build()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_domain_names_pass() {
        assert!(validate_domain_name("billing").is_ok());
        assert!(validate_domain_name("my-domain").is_ok());
        assert!(validate_domain_name("a1").is_ok());
        assert!(validate_domain_name("domain-123-test").is_ok());
    }

    #[test]
    fn rejects_name_too_short() {
        assert!(validate_domain_name("a").is_err());
    }

    #[test]
    fn rejects_name_too_long() {
        let long_name = "a".repeat(65);
        assert!(validate_domain_name(&long_name).is_err());
    }

    #[test]
    fn accepts_name_at_boundary_lengths() {
        assert!(validate_domain_name("ab").is_ok());
        assert!(validate_domain_name(&"a".repeat(64)).is_ok());
    }

    #[test]
    fn rejects_uppercase_characters() {
        assert!(validate_domain_name("Billing").is_err());
        assert!(validate_domain_name("MY-DOMAIN").is_err());
    }

    #[test]
    fn rejects_special_characters() {
        assert!(validate_domain_name("billing@v2").is_err());
        assert!(validate_domain_name("my_domain").is_err());
        assert!(validate_domain_name("domain.name").is_err());
    }

    #[test]
    fn rejects_leading_hyphen() {
        assert!(validate_domain_name("-billing").is_err());
    }

    #[test]
    fn rejects_trailing_hyphen() {
        assert!(validate_domain_name("billing-").is_err());
    }

    #[test]
    fn rejects_reserved_konductor_domain_name() {
        let err = validate_domain_name("konductor").unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("reserved"),
            "error should mention reserved: {err}"
        );
    }

    #[test]
    fn validate_environment_name_accepts_simple_names() {
        assert!(validate_environment_name("dev").is_ok());
        assert!(validate_environment_name("staging").is_ok());
        assert!(validate_environment_name("prod").is_ok());
        assert!(validate_environment_name("qa-1").is_ok());
        assert!(validate_environment_name("a").is_ok());
    }

    #[test]
    fn validate_environment_name_rejects_invalid_chars() {
        assert!(validate_environment_name("DEV").is_err());
        assert!(validate_environment_name("dev_1").is_err());
        assert!(validate_environment_name("dev.prod").is_err());
        assert!(validate_environment_name("").is_err());
    }

    #[test]
    fn validate_environment_name_rejects_leading_hyphen() {
        assert!(validate_environment_name("-dev").is_err());
    }

    #[test]
    fn validate_environment_name_rejects_too_long() {
        assert!(validate_environment_name(&"a".repeat(33)).is_err());
        assert!(validate_environment_name(&"a".repeat(32)).is_ok());
    }

    #[test]
    fn parse_environments_splits_and_trims() {
        let envs = parse_environments(" dev , staging,prod ").expect("should parse");
        assert_eq!(envs, vec!["dev", "staging", "prod"]);
    }

    #[test]
    fn parse_environments_requires_at_least_one() {
        assert!(parse_environments("").is_err());
        assert!(parse_environments(",,,").is_err());
    }

    #[test]
    fn parse_environments_rejects_invalid_entry() {
        assert!(parse_environments("dev,PROD").is_err());
        assert!(parse_environments("dev,-bad").is_err());
    }

    #[test]
    fn parse_environments_accepts_single_env() {
        let envs = parse_environments("dev").expect("should parse");
        assert_eq!(envs, vec!["dev"]);
    }

    mod handle_tests {
        use super::*;
        use aws_sdk_dynamodb::operation::transact_write_items::{
            TransactWriteItemsError, TransactWriteItemsOutput,
        };
        use aws_sdk_dynamodb::types::error::TransactionCanceledException;
        use aws_sdk_dynamodb::types::CancellationReason;
        use aws_sdk_eventbridge::operation::put_events::PutEventsOutput;
        use aws_sdk_sfn::operation::start_execution::StartExecutionOutput;
        use aws_smithy_mocks::{mock, mock_client, Rule};
        use serde_json::Value;

        const ISSUER: &str = "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_AbCdEfGh";

        fn valid_request() -> RegisterDomainRequest {
            RegisterDomainRequest {
                domain_name: "billing".to_string(),
                description: None,
                owner_account_id: "111122223333".to_string(),
                owner_email: "owner@example.com".to_string(),
                secondary_owner_emails: vec!["backup@example.com".to_string()],
                cost_center: None,
                owning_team: None,
                environment_accounts: None,
            }
        }

        fn oauth(groups: &[&str]) -> TrustedPrincipal {
            TrustedPrincipal::oauth(
                "a1b2c3d4-1111-2222-3333-444455556666",
                ISSUER,
                "client123",
                vec![],
                groups.iter().map(|g| g.to_string()).collect(),
            )
        }

        fn set_env() {
            env::set_var("DOMAINS_TABLE", "domains");
            env::set_var(
                "APPROVAL_STATE_MACHINE_ARN",
                "arn:aws:states:us-east-1:123:stateMachine:approval",
            );
            env::set_var("KONDUKTOR_EVENT_BUS", "konductor-bus");
            env::set_var("ENVIRONMENTS", "dev,staging,prod");
            env::set_var("ORGANIZATION", "acme");
        }

        use crate::register::test_rules::{existing_bus_rule, no_bus_rule, no_param_rule};

        fn sfn_ok_rule() -> Rule {
            mock!(aws_sdk_sfn::Client::start_execution).then_output(|| {
                StartExecutionOutput::builder()
                    .execution_arn("arn:exec")
                    .start_date(aws_sdk_sfn::primitives::DateTime::from_secs(0))
                    .build()
                    .unwrap()
            })
        }

        fn tx_ok_rule() -> Rule {
            mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .then_output(|| TransactWriteItemsOutput::builder().build())
        }

        fn cancelled(reasons: Vec<CancellationReason>) -> TransactWriteItemsError {
            TransactWriteItemsError::TransactionCanceledException(
                TransactionCanceledException::builder()
                    .set_cancellation_reasons(Some(reasons))
                    .message("cancelled")
                    .build(),
            )
        }

        /// The domain-row put failed its condition; the counter update was
        /// satisfied. Mirrors DynamoDB's per-item cancellation reasons.
        fn name_taken_reasons(status: &str, request_id: &str) -> Vec<CancellationReason> {
            vec![
                CancellationReason::builder()
                    .code("ConditionalCheckFailed")
                    .item("domainName", s("billing"))
                    .item("requestId", s(request_id))
                    .item("status", s(status))
                    .item("createdAt", s("2024-01-01T00:00:00+00:00"))
                    .build(),
                CancellationReason::builder().build(),
            ]
        }

        /// The domain-row put was satisfied; the counter update failed its
        /// below-cap condition.
        fn at_cap_reasons() -> Vec<CancellationReason> {
            vec![
                CancellationReason::builder().build(),
                CancellationReason::builder()
                    .code("ConditionalCheckFailed")
                    .message("domain-count cap")
                    .build(),
            ]
        }

        fn ok_eb_rule() -> Rule {
            mock!(aws_sdk_eventbridge::Client::put_events)
                .then_output(|| PutEventsOutput::builder().build())
        }

        fn failed_event_rule() -> Rule {
            mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries()
                        .iter()
                        .any(|e| e.detail_type() == Some("RegisterDomainFailed"))
                })
                .then_output(|| PutEventsOutput::builder().build())
        }

        fn succeeded_event_rule() -> Rule {
            mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries()
                        .iter()
                        .any(|e| e.detail_type() == Some("RegisterDomainSucceeded"))
                })
                .then_output(|| PutEventsOutput::builder().build())
        }

        fn iam_principal() -> TrustedPrincipal {
            TrustedPrincipal::iam("111122223333", None)
        }

        #[tokio::test]
        async fn collision_writes_system_rejection_and_charges_the_counter() {
            set_env();

            let captured: std::sync::Arc<std::sync::Mutex<Vec<(String, usize)>>> =
                std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let captured2 = std::sync::Arc::clone(&captured);
            let tx_rule = mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .match_requests(move |req| {
                    let items = req.transact_items();
                    let status = items
                        .first()
                        .and_then(|t| t.put())
                        .and_then(|p| p.item().get("status"))
                        .and_then(|v| v.as_s().ok())
                        .cloned()
                        .unwrap_or_default();
                    captured2.lock().unwrap().push((status, items.len()));
                    true
                })
                .then_output(|| TransactWriteItemsOutput::builder().build());
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let bus_exists = existing_bus_rule();
            let failed_rule = failed_event_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&bus_exists, &failed_rule]
            );

            let sfn_rule = sfn_ok_rule();
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &valid_request(), &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(sfn_rule.num_calls(), 0, "no approval workflow on collision");
            assert_eq!(failed_rule.num_calls(), 1, "RegisterDomainFailed emitted");
            let recorded = captured.lock().unwrap();
            assert_eq!(
                recorded.first(),
                Some(&("REJECTED".to_string(), 2)),
                "rejected row and counter increment commit in one transaction"
            );
        }

        #[tokio::test]
        async fn collision_rejection_names_colliders_and_marks_system_decision() {
            set_env();

            let captured: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
                std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let captured2 = std::sync::Arc::clone(&captured);
            let tx_rule = mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .match_requests(move |req| {
                    let item = req
                        .transact_items()
                        .first()
                        .and_then(|t| t.put())
                        .map(|p| p.item().clone())
                        .unwrap_or_default();
                    let decided_by = get_s(&item, "decidedBy").unwrap_or_default();
                    let justification = get_s(&item, "justification").unwrap_or_default();
                    captured2
                        .lock()
                        .unwrap()
                        .push(format!("{decided_by}|{justification}"));
                    true
                })
                .then_output(|| TransactWriteItemsOutput::builder().build());
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let bus_exists = existing_bus_rule();
            let failed_rule = ok_eb_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&bus_exists, &failed_rule]
            );
            let sfn = mock_client!(aws_sdk_sfn, &[]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &valid_request(), &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            let recorded = captured.lock().unwrap();
            let first = recorded.first().cloned().unwrap_or_default();
            let (decided_by, justification) = first.split_once('|').unwrap_or(("", ""));
            assert_eq!(decided_by, "system");
            assert!(
                justification.contains("acme-dev-billing-event-bus"),
                "justification names the colliding resource: {justification}"
            );
        }

        #[tokio::test]
        async fn collision_over_rejected_row_overwrites_without_charging_the_counter() {
            set_env();

            let shapes: std::sync::Arc<std::sync::Mutex<Vec<(String, usize)>>> =
                std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let shapes2 = std::sync::Arc::clone(&shapes);
            let record_shape = move |req: &aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsInput| {
                let items = req.transact_items();
                let condition = items
                    .first()
                    .and_then(|t| t.put())
                    .and_then(|p| p.condition_expression())
                    .unwrap_or("")
                    .to_string();
                shapes2.lock().unwrap().push((condition, items.len()));
                true
            };
            let taken = mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .then_error(|| cancelled(name_taken_reasons("REJECTED", "someone-elses-request")));
            let overwrite = mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .match_requests(record_shape)
                .then_output(|| TransactWriteItemsOutput::builder().build());
            let ddb = mock_client!(aws_sdk_dynamodb, [&taken, &overwrite]);

            let bus_exists = existing_bus_rule();
            let failed_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries().iter().any(|e| {
                        e.detail_type() == Some("RegisterDomainFailed")
                            && e.detail()
                                .is_some_and(|d| d.contains("resource name collision"))
                    })
                })
                .then_output(|| PutEventsOutput::builder().build());
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&bus_exists, &failed_rule]
            );
            let sfn = mock_client!(aws_sdk_sfn, &[]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &valid_request(), &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(
                failed_rule.num_calls(),
                1,
                "collision failure still reported"
            );
            assert_eq!(
                shapes.lock().unwrap().as_slice(),
                &[("#status = :rejected".to_string(), 1)],
                "overwrite carries no counter increment"
            );
        }

        #[tokio::test]
        async fn collision_check_error_fails_the_command_without_writing() {
            set_env();

            let throttled = mock!(aws_sdk_eventbridge::Client::describe_event_bus).then_error(|| {
                aws_sdk_eventbridge::operation::describe_event_bus::DescribeEventBusError::unhandled(
                    "throttled",
                )
            });
            let tx_rule = tx_ok_rule();
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);
            let failed_rule = ok_eb_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&throttled, &failed_rule]
            );
            let sfn = mock_client!(aws_sdk_sfn, &[]);
            let ssm = mock_client!(aws_sdk_ssm, &[]);

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &valid_request(), &iam_principal(), "msg-1").await;

            assert!(result.is_err(), "check errors propagate to queue redrive");
            assert_eq!(tx_rule.num_calls(), 0, "no record written on a guess");
        }

        #[tokio::test]
        async fn creates_domain_and_starts_approval() {
            set_env();

            let tx_rule = tx_ok_rule();
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let eb_rule = ok_eb_rule();
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );

            let sfn_rule = sfn_ok_rule();
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &valid_request(), &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(tx_rule.num_calls(), 1);
            assert_eq!(sfn_rule.num_calls(), 1);
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn new_row_and_counter_charge_commit_in_one_conditioned_transaction() {
            set_env();

            // The concurrency guarantee: the row put requires the name to be
            // absent and the counter update requires headroom below the cap,
            // in one transaction — so two registrations racing for the final
            // slot serialize on the counter and exactly one commits.
            let tx_rule = mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .match_requests(|req| {
                    let items = req.transact_items();
                    if items.len() != 2 {
                        return false;
                    }
                    let put_ok = items[0].put().is_some_and(|p| {
                        p.condition_expression() == Some("attribute_not_exists(domainName)")
                    });
                    let counter_ok = items[1].update().is_some_and(|u| {
                        u.key().get("domainName").and_then(|v| v.as_s().ok())
                            == Some(
                                &konductor_common::dynamodb::DOMAIN_COUNT_COUNTER_KEY.to_string(),
                            )
                            && u.condition_expression()
                                .is_some_and(|c| c.contains("#count < :cap"))
                    });
                    put_ok && counter_ok
                })
                .then_output(|| TransactWriteItemsOutput::builder().build());
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let eb_rule = ok_eb_rule();
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );
            let sfn_rule = sfn_ok_rule();
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &valid_request(), &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(tx_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn at_cap_registration_fails_with_limit_reason_and_no_retry() {
            set_env();

            let tx_rule = mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .then_error(|| cancelled(at_cap_reasons()));
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let failed_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries().iter().any(|e| {
                        e.detail_type() == Some("RegisterDomainFailed")
                            && e.detail().is_some_and(|d| d.contains("limit exceeded"))
                    })
                })
                .then_output(|| PutEventsOutput::builder().build());
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &failed_rule]
            );
            let sfn_rule = sfn_ok_rule();
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &valid_request(), &iam_principal(), "msg-1").await;

            assert!(
                result.is_ok(),
                "cap failure is deterministic: message acked, no retry"
            );
            assert_eq!(
                failed_rule.num_calls(),
                1,
                "RegisterDomainFailed carries the limit reason"
            );
            assert_eq!(sfn_rule.num_calls(), 0, "no approval workflow at the cap");
        }

        #[tokio::test]
        async fn at_cap_collision_rejection_fails_with_limit_reason() {
            set_env();

            let tx_rule = mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .then_error(|| cancelled(at_cap_reasons()));
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let bus_exists = existing_bus_rule();
            let failed_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries().iter().any(|e| {
                        e.detail_type() == Some("RegisterDomainFailed")
                            && e.detail().is_some_and(|d| d.contains("limit exceeded"))
                    })
                })
                .then_output(|| PutEventsOutput::builder().build());
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&bus_exists, &failed_rule]
            );
            let sfn_rule = sfn_ok_rule();
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &valid_request(), &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(failed_rule.num_calls(), 1);
            assert_eq!(sfn_rule.num_calls(), 0);
        }

        #[tokio::test]
        async fn oauth_registration_with_owning_team_is_accepted() {
            set_env();

            let tx_rule = mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .match_requests(|req| {
                    req.transact_items()
                        .first()
                        .and_then(|t| t.put())
                        .and_then(|p| p.item().get("owningTeam"))
                        .and_then(|v| v.as_s().ok())
                        .is_some_and(|v| v == "team-payments")
                })
                .then_output(|| TransactWriteItemsOutput::builder().build());
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);
            let eb_rule = succeeded_event_rule();
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );
            let sfn_rule = sfn_ok_rule();
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let mut request = valid_request();
            request.owning_team = Some("team-payments".to_string());
            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(
                &clients,
                &request,
                &oauth(&["team-payments", "team-platform"]),
                "msg-1",
            )
            .await;

            assert!(result.is_ok());
            assert_eq!(tx_rule.num_calls(), 1);
            assert_eq!(sfn_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn oauth_registration_without_owning_team_fails() {
            set_env();

            let ddb = mock_client!(aws_sdk_dynamodb, &[]);
            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries().iter().any(|e| {
                        e.detail_type() == Some("RegisterDomainFailed")
                            && e.detail().is_some_and(|d| d.contains("owningTeam"))
                    })
                })
                .then_output(|| PutEventsOutput::builder().build());
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );
            let sfn = mock_client!(aws_sdk_sfn, &[]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(
                &clients,
                &valid_request(),
                &oauth(&["team-payments"]),
                "msg-1",
            )
            .await;

            assert!(result.is_ok(), "denial is reported via the failed event");
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn oauth_registration_with_foreign_or_platform_team_fails() {
            set_env();

            for owning_team in ["team-other", "platform-admin"] {
                let ddb = mock_client!(aws_sdk_dynamodb, &[]);
                let eb_rule = failed_event_rule();
                let no_bus = no_bus_rule();
                let eb = mock_client!(
                    aws_sdk_eventbridge,
                    aws_smithy_mocks::RuleMode::MatchAny,
                    [&no_bus, &eb_rule]
                );
                let sfn = mock_client!(aws_sdk_sfn, &[]);
                let no_param = no_param_rule();
                let ssm = mock_client!(
                    aws_sdk_ssm,
                    aws_smithy_mocks::RuleMode::MatchAny,
                    [&no_param]
                );

                let mut request = valid_request();
                request.owning_team = Some(owning_team.to_string());
                let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
                let result = handle(&clients, &request, &oauth(&["team-payments"]), "msg-1").await;

                assert!(result.is_ok(), "{owning_team}: denial via failed event");
                assert_eq!(eb_rule.num_calls(), 1, "{owning_team}");
            }
        }

        #[tokio::test]
        async fn rejects_duplicate_domain_with_failed_event() {
            set_env();

            let tx_rule = mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .then_error(|| cancelled(name_taken_reasons("ACTIVE", "someone-elses-request")));
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries().iter().any(|entry| {
                        entry.detail_type() == Some("RegisterDomainFailed")
                            && entry.detail().is_some_and(|d| {
                                serde_json::from_str::<Value>(d).is_ok_and(|v| {
                                    v.get("secondaryOwnerEmails")
                                        == Some(&json!(["backup@example.com"]))
                                })
                            })
                    })
                })
                .then_output(|| PutEventsOutput::builder().build());
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );

            let sfn = mock_client!(aws_sdk_sfn, &[]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &valid_request(), &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(tx_rule.num_calls(), 1);
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn retry_of_own_registration_resumes_after_conditional_failure() {
            set_env();

            let tx_rule = mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .then_error(|| cancelled(name_taken_reasons("PENDING_APPROVAL", "msg-1")));
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries().iter().any(|entry| {
                        entry.detail_type() == Some("RegisterDomainSucceeded")
                            && entry
                                .detail()
                                .is_some_and(|d| d.contains("2024-01-01T00:00:00+00:00"))
                    })
                })
                .then_output(|| PutEventsOutput::builder().build());
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );

            let sfn_rule = sfn_ok_rule();
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &valid_request(), &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(
                tx_rule.num_calls(),
                1,
                "no second transaction on a live own row"
            );
            assert_eq!(sfn_rule.num_calls(), 1);
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn tolerates_execution_already_exists_on_retry() {
            set_env();

            let tx_rule = tx_ok_rule();
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let eb_rule = ok_eb_rule();
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );

            let sfn_rule = mock!(aws_sdk_sfn::Client::start_execution).then_error(|| {
                aws_sdk_sfn::operation::start_execution::StartExecutionError::ExecutionAlreadyExists(
                    aws_sdk_sfn::types::error::ExecutionAlreadyExists::builder().build(),
                )
            });
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &valid_request(), &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(sfn_rule.num_calls(), 1);
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn validation_failure_leaves_the_row_and_counter_untouched() {
            set_env();

            // No DynamoDB rules: any transact_write_items call fails the mocked
            // client, proving a validation failure never reaches the write path.
            let ddb = mock_client!(aws_sdk_dynamodb, &[]);

            let eb_rule = ok_eb_rule();
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );

            let sfn = mock_client!(aws_sdk_sfn, &[]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let request = RegisterDomainRequest {
                domain_name: "INVALID".to_string(),
                ..valid_request()
            };

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &request, &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn creates_domain_with_optional_description() {
            set_env();

            let tx_rule = mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .match_requests(|req| {
                    req.transact_items()
                        .first()
                        .and_then(|t| t.put())
                        .is_some_and(|p| p.item().contains_key("description"))
                })
                .then_output(|| TransactWriteItemsOutput::builder().build());
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let eb_rule = ok_eb_rule();
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );

            let sfn_rule = sfn_ok_rule();
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let request = RegisterDomainRequest {
                description: Some("Billing domain events".to_string()),
                ..valid_request()
            };

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &request, &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(tx_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn stamps_environments_list_on_the_row() {
            set_env();

            let tx_rule = mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .match_requests(|req| {
                    let Some(put) = req.transact_items().first().and_then(|t| t.put()) else {
                        return false;
                    };
                    let Some(AttributeValue::L(values)) = put.item().get("environments") else {
                        return false;
                    };
                    let parsed: Vec<String> = values
                        .iter()
                        .filter_map(|v| v.as_s().ok().cloned())
                        .collect();
                    parsed == vec!["dev", "staging", "prod"]
                })
                .then_output(|| TransactWriteItemsOutput::builder().build());
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let eb_rule = ok_eb_rule();
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );

            let sfn_rule = sfn_ok_rule();
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &valid_request(), &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(tx_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn includes_environments_in_succeeded_event() {
            set_env();

            let tx_rule = tx_ok_rule();
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let eb_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries().iter().any(|entry| {
                        if entry.detail_type() != Some("RegisterDomainSucceeded") {
                            return false;
                        }
                        let Some(detail) = entry.detail() else {
                            return false;
                        };
                        let Ok(value) = serde_json::from_str::<Value>(detail) else {
                            return false;
                        };
                        value.get("environments") == Some(&json!(["dev", "staging", "prod"]))
                    })
                })
                .then_output(|| PutEventsOutput::builder().build());
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );

            let sfn_rule = sfn_ok_rule();
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &valid_request(), &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn reregistration_over_rejected_succeeds_without_charging_the_counter() {
            set_env();

            // A REJECTED record owns the name: the first transaction aborts on
            // the name-absent condition, and the overwrite transaction replaces
            // the record wholesale without touching the counter (the slot was
            // charged when the rejected row was created).
            let shapes: std::sync::Arc<std::sync::Mutex<Vec<(String, usize)>>> =
                std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let shapes2 = std::sync::Arc::clone(&shapes);
            let record_shape = move |req: &aws_sdk_dynamodb::operation::transact_write_items::TransactWriteItemsInput| {
                let items = req.transact_items();
                let condition = items
                    .first()
                    .and_then(|t| t.put())
                    .and_then(|p| p.condition_expression())
                    .unwrap_or("")
                    .to_string();
                shapes2.lock().unwrap().push((condition, items.len()));
                true
            };
            let taken = mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .then_error(|| cancelled(name_taken_reasons("REJECTED", "someone-elses-request")));
            let overwrite = mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .match_requests(record_shape)
                .then_output(|| TransactWriteItemsOutput::builder().build());
            let ddb = mock_client!(aws_sdk_dynamodb, [&taken, &overwrite]);

            let eb_rule = succeeded_event_rule();
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );

            let sfn_rule = sfn_ok_rule();
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &valid_request(), &iam_principal(), "msg-2").await;

            assert!(result.is_ok());
            assert_eq!(sfn_rule.num_calls(), 1);
            assert_eq!(eb_rule.num_calls(), 1);
            assert_eq!(
                shapes.lock().unwrap().as_slice(),
                &[("#status = :rejected".to_string(), 1)],
                "re-registration overwrites without a counter increment"
            );
        }

        #[tokio::test]
        async fn duplicate_over_pending_approval_still_emits_failed_event() {
            set_env();

            // A non-REJECTED record (e.g. PENDING_APPROVAL) blocks
            // registration; the live-row classification emits
            // RegisterDomainFailed with "domain already exists".
            let tx_rule = mock!(aws_sdk_dynamodb::Client::transact_write_items).then_error(|| {
                cancelled(name_taken_reasons(
                    "PENDING_APPROVAL",
                    "someone-elses-request",
                ))
            });
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let eb_rule = failed_event_rule();
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );

            let sfn = mock_client!(aws_sdk_sfn, &[]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &valid_request(), &iam_principal(), "msg-2").await;

            assert!(result.is_ok());
            assert_eq!(tx_rule.num_calls(), 1);
            assert_eq!(eb_rule.num_calls(), 1);
            // sfn has no mock rules — any call would error, so Ok(()) proves
            // the workflow was not started for a duplicate registration.
        }

        fn default_effective_map() -> Value {
            json!({
                "dev": "111122223333",
                "staging": "111122223333",
                "prod": "111122223333"
            })
        }

        fn sfn_input_rule(expected_accounts: Value) -> Rule {
            mock!(aws_sdk_sfn::Client::start_execution)
                .match_requests(move |req| {
                    req.input()
                        .and_then(|i| serde_json::from_str::<Value>(i).ok())
                        .is_some_and(|input| {
                            input.get("environmentAccounts") == Some(&expected_accounts)
                        })
                })
                .then_output(|| {
                    StartExecutionOutput::builder()
                        .execution_arn("arn:exec")
                        .start_date(aws_sdk_sfn::primitives::DateTime::from_secs(0))
                        .build()
                        .unwrap()
                })
        }

        fn succeeded_event_with_accounts_rule(expected_accounts: Value) -> Rule {
            mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(move |req| {
                    req.entries().iter().any(|entry| {
                        entry.detail_type() == Some("RegisterDomainSucceeded")
                            && entry
                                .detail()
                                .and_then(|d| serde_json::from_str::<Value>(d).ok())
                                .is_some_and(|v| {
                                    v.get("environmentAccounts") == Some(&expected_accounts)
                                })
                    })
                })
                .then_output(|| PutEventsOutput::builder().build())
        }

        #[tokio::test]
        async fn mapless_registration_resolves_everywhere_to_the_default_account() {
            set_env();

            let tx_rule = mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .match_requests(|req| {
                    req.transact_items()
                        .first()
                        .and_then(|t| t.put())
                        .is_some_and(|p| !p.item().contains_key("environmentAccounts"))
                })
                .then_output(|| TransactWriteItemsOutput::builder().build());
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let eb_rule = succeeded_event_with_accounts_rule(default_effective_map());
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );
            let sfn_rule = sfn_input_rule(default_effective_map());
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &valid_request(), &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(tx_rule.num_calls(), 1, "record stores no map attribute");
            assert_eq!(
                sfn_rule.num_calls(),
                1,
                "workflow input carries the full default map"
            );
            assert_eq!(eb_rule.num_calls(), 1, "event carries the full default map");
        }

        #[tokio::test]
        async fn sparse_override_is_stored_sparse_and_resolved_into_a_full_effective_map() {
            set_env();

            let tx_rule = mock!(aws_sdk_dynamodb::Client::transact_write_items)
                .match_requests(|req| {
                    req.transact_items()
                        .first()
                        .and_then(|t| t.put())
                        .and_then(|p| p.item().get("environmentAccounts"))
                        .and_then(|v| v.as_m().ok())
                        .is_some_and(|m| {
                            m.len() == 1
                                && m.get("prod")
                                    .and_then(|v| v.as_s().ok())
                                    .map(String::as_str)
                                    == Some("999988887777")
                        })
                })
                .then_output(|| TransactWriteItemsOutput::builder().build());
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let effective = json!({
                "dev": "111122223333",
                "staging": "111122223333",
                "prod": "999988887777"
            });
            let eb_rule = succeeded_event_with_accounts_rule(effective.clone());
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );
            let sfn_rule = sfn_input_rule(effective);
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let mut request = valid_request();
            request.environment_accounts = Some(HashMap::from([(
                "prod".to_string(),
                "999988887777".to_string(),
            )]));
            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &request, &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(
                tx_rule.num_calls(),
                1,
                "record stores only the sparse override"
            );
            assert_eq!(sfn_rule.num_calls(), 1);
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn every_environment_may_be_overridden() {
            set_env();

            let tx_rule = tx_ok_rule();
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let effective = json!({
                "dev": "222233334444",
                "staging": "555566667777",
                "prod": "999988887777"
            });
            let eb_rule = succeeded_event_with_accounts_rule(effective.clone());
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );
            let sfn_rule = sfn_input_rule(effective);
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let mut request = valid_request();
            request.environment_accounts = Some(HashMap::from([
                ("dev".to_string(), "222233334444".to_string()),
                ("staging".to_string(), "555566667777".to_string()),
                ("prod".to_string(), "999988887777".to_string()),
            ]));
            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &request, &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(
                sfn_rule.num_calls(),
                1,
                "no environment keeps the default account"
            );
            assert_eq!(eb_rule.num_calls(), 1);
        }

        #[tokio::test]
        async fn unknown_environment_key_fails_before_any_write_or_workflow() {
            set_env();

            let ddb = mock_client!(aws_sdk_dynamodb, &[]);
            let failed_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries().iter().any(|e| {
                        e.detail_type() == Some("RegisterDomainFailed")
                            && e.detail().is_some_and(|d| d.contains("'qa'"))
                    })
                })
                .then_output(|| PutEventsOutput::builder().build());
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &failed_rule]
            );
            let sfn = mock_client!(aws_sdk_sfn, &[]);
            let ssm = mock_client!(aws_sdk_ssm, &[]);

            let mut request = valid_request();
            request.environment_accounts = Some(HashMap::from([(
                "qa".to_string(),
                "999988887777".to_string(),
            )]));
            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &request, &iam_principal(), "msg-1").await;

            assert!(result.is_ok(), "deterministic rejection acks the message");
            assert_eq!(failed_rule.num_calls(), 1, "reason names the offending key");
        }

        #[tokio::test]
        async fn malformed_account_value_fails_before_any_write_or_workflow() {
            set_env();

            let ddb = mock_client!(aws_sdk_dynamodb, &[]);
            let failed_rule = mock!(aws_sdk_eventbridge::Client::put_events)
                .match_requests(|req| {
                    req.entries().iter().any(|e| {
                        e.detail_type() == Some("RegisterDomainFailed")
                            && e.detail().is_some_and(|d| d.contains("not-an-account"))
                    })
                })
                .then_output(|| PutEventsOutput::builder().build());
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &failed_rule]
            );
            let sfn = mock_client!(aws_sdk_sfn, &[]);
            let ssm = mock_client!(aws_sdk_ssm, &[]);

            let mut request = valid_request();
            request.environment_accounts = Some(HashMap::from([(
                "prod".to_string(),
                "not-an-account".to_string(),
            )]));
            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &request, &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(
                failed_rule.num_calls(),
                1,
                "reason names the offending value"
            );
        }

        #[tokio::test]
        async fn duplicate_accounts_across_environments_are_accepted() {
            set_env();

            let tx_rule = tx_ok_rule();
            let ddb = mock_client!(aws_sdk_dynamodb, [&tx_rule]);

            let effective = json!({
                "dev": "111122223333",
                "staging": "999988887777",
                "prod": "999988887777"
            });
            let eb_rule = succeeded_event_with_accounts_rule(effective.clone());
            let no_bus = no_bus_rule();
            let eb = mock_client!(
                aws_sdk_eventbridge,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_bus, &eb_rule]
            );
            let sfn_rule = sfn_input_rule(effective);
            let sfn = mock_client!(aws_sdk_sfn, [&sfn_rule]);
            let no_param = no_param_rule();
            let ssm = mock_client!(
                aws_sdk_ssm,
                aws_smithy_mocks::RuleMode::MatchAny,
                [&no_param]
            );

            let mut request = valid_request();
            request.environment_accounts = Some(HashMap::from([
                ("staging".to_string(), "999988887777".to_string()),
                ("prod".to_string(), "999988887777".to_string()),
            ]));
            let clients = crate::handler::Clients { ddb, eb, sfn, ssm };
            let result = handle(&clients, &request, &iam_principal(), "msg-1").await;

            assert!(result.is_ok());
            assert_eq!(tx_rule.num_calls(), 1);
            assert_eq!(sfn_rule.num_calls(), 1);
            assert_eq!(eb_rule.num_calls(), 1);
        }
    }
}
