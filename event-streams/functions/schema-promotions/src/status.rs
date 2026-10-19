//! `GET /schemas/promotions`: the head item's status view through a
//! consistent read. Callers hold `schema:promote` or a platform role.

use serde_json::Value;

use konductor_common::api;
use konductor_common::dynamodb::s;
use konductor_common::error::ControlPlaneError;
use konductor_common::grant_engine::actions;
use konductor_common::principal::{
    AuthorizationDecision, GlobalRole, ResourceAuthorizer, TrustedPrincipal,
};
use konductor_common::promotion::{promotion_pk, PROMOTION_SK};
use konductor_common::promotion_store::promotion_from_item;

use crate::handler::Clients;
use crate::support::{promotions_table, require_active_domain, source_target, stage_of};

pub(crate) async fn promotion_status(
    clients: &Clients,
    authorizer: &impl ResourceAuthorizer,
    principal: &TrustedPrincipal,
    event: &Value,
) -> Result<Value, ControlPlaneError> {
    let domain = api::query_param(event, "domain")
        .ok_or_else(|| api::validation_error("domain query parameter is required"))?;
    let event_name = api::query_param(event, "eventName")
        .ok_or_else(|| api::validation_error("eventName query parameter is required"))?;
    let (_source, target) = source_target(stage_of(event)?)?;
    let platform_reader = principal
        .global_roles()
        .iter()
        .any(|r| matches!(r, GlobalRole::PlatformAdmin | GlobalRole::PlatformAuditor));
    let granted = authorizer
        .authorize(principal, actions::SCHEMA_PROMOTE, &domain)
        .await
        == AuthorizationDecision::Allow;
    if !platform_reader && !granted {
        return Err(ControlPlaneError::AccessDenied);
    }
    require_active_domain(clients, &domain).await?;
    let output = clients
        .ddb
        .get_item()
        .table_name(promotions_table()?)
        .key("pk", s(&promotion_pk(&domain, &event_name, &target)))
        .key("sk", s(PROMOTION_SK))
        .consistent_read(true)
        .send()
        .await
        .map_err(|e| {
            let err = e.into_service_error();
            tracing::error!(error = %err, "promotion head read failed");
            ControlPlaneError::DynamoDb(err.to_string())
        })?;
    let item = output
        .item
        .ok_or_else(|| ControlPlaneError::PromotionNotFound {
            domain: domain.clone(),
            schema_name: event_name.clone(),
            environment: target.clone(),
        })?;
    let promotion = promotion_from_item(&item)
        .ok_or_else(|| ControlPlaneError::DynamoDb("malformed promotion item".to_string()))?;
    Ok(api::success_response(200, &promotion.view()))
}
