//! Reconstruction of [`TrustedPrincipal`] from the trusted propagation
//! surfaces: SQS message attributes written by the API Gateway mapping
//! templates, and the API Gateway proxy `requestContext` seen by Lambda
//! proxy handlers.
//!
//! The mapping templates source every identity field from API Gateway
//! authorization context (`$context.authorizer.*` for OAuth,
//! `$context.identity.*` for IAM), never from the request body, so a caller
//! cannot assert a foreign identity. These parsers fail closed: a missing
//! discriminator, an unsupported envelope version, an unknown mechanism, or a
//! malformed field is an error, never a silently reinterpreted identity.

use std::collections::HashMap;

use serde_json::Value;

use crate::principal::{GlobalRole, TrustedPrincipal, TRUSTED_PRINCIPAL_VERSION};

/// Parses the trusted principal from the proxy `requestContext` and emits a
/// structured log line. Returns the parsed principal so callers that need the
/// identity (e.g. for ownership checks) can use it; callers that only need
/// audit provenance can discard the result.
pub fn log_principal(request_context: &Value) -> CtxResult<TrustedPrincipal> {
    let result = TrustedPrincipal::from_proxy_request_context(request_context);
    match &result {
        Ok(p) => tracing::info!(
            mechanism = ?p.authentication_mechanism(),
            principal_id = %p.principal_id(),
            "authenticated principal"
        ),
        Err(e) => tracing::warn!(error = %e, "trusted-principal context unavailable"),
    }
    result
}

/// SQS message-attribute names that carry the versioned trusted-principal
/// envelope alongside the command payload body.
pub mod attr {
    pub const VERSION: &str = "PrincipalVersion";
    pub const MECHANISM: &str = "AuthMechanism";
    pub const PRINCIPAL_ID: &str = "PrincipalId";
    pub const ISSUER: &str = "Issuer";
    pub const CLIENT_ID: &str = "ClientId";
    pub const GLOBAL_ROLES: &str = "GlobalRoles";
    pub const TEAM_GROUPS: &str = "TeamGroups";
    pub const ACCOUNT_ID: &str = "AccountId";
    pub const PRINCIPAL_ARN: &str = "PrincipalArn";
}

/// A failure to reconstruct a [`TrustedPrincipal`] from a propagation surface.
///
/// Every variant is a fail-closed outcome: callers must treat it as an
/// authorization denial, never as an anonymous or default identity.
#[derive(Debug, thiserror::Error)]
pub enum PrincipalContextError {
    #[error("missing trusted-principal field: {0}")]
    MissingField(&'static str),
    #[error("unsupported trusted-principal envelope version: {0}")]
    UnsupportedVersion(String),
    #[error("unknown authentication mechanism: {0}")]
    UnknownMechanism(String),
    #[error("malformed trusted-principal field {field}: {reason}")]
    MalformedField { field: &'static str, reason: String },
    #[error("no trusted principal present in request context")]
    MissingContext,
}

type CtxResult<T> = Result<T, PrincipalContextError>;

fn required<'a>(attrs: &'a HashMap<String, String>, name: &'static str) -> CtxResult<&'a str> {
    attrs
        .get(name)
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(PrincipalContextError::MissingField(name))
}

fn parse_json_list<T>(raw: Option<&str>, field: &'static str) -> CtxResult<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    match raw {
        None => Ok(Vec::new()),
        Some(s) if s.trim().is_empty() => Ok(Vec::new()),
        Some(s) => {
            serde_json::from_str::<Vec<T>>(s).map_err(|e| PrincipalContextError::MalformedField {
                field,
                reason: e.to_string(),
            })
        }
    }
}

fn parse_global_roles(raw: Option<&str>) -> CtxResult<Vec<GlobalRole>> {
    parse_json_list(raw, attr::GLOBAL_ROLES)
}

fn parse_team_groups(raw: Option<&str>) -> CtxResult<Vec<String>> {
    parse_json_list(raw, attr::TEAM_GROUPS)
}

fn check_version(raw: Option<&String>) -> CtxResult<()> {
    let version = raw.ok_or(PrincipalContextError::MissingField(attr::VERSION))?;
    if *version == TRUSTED_PRINCIPAL_VERSION.to_string() {
        Ok(())
    } else {
        Err(PrincipalContextError::UnsupportedVersion(version.clone()))
    }
}

fn build_oauth(
    principal_id: &str,
    issuer: &str,
    client_id: &str,
    global_roles: Vec<GlobalRole>,
    team_groups: Vec<String>,
) -> TrustedPrincipal {
    TrustedPrincipal::oauth(principal_id, issuer, client_id, global_roles, team_groups)
}

fn build_iam(account_id: &str, principal_arn: Option<&str>) -> TrustedPrincipal {
    let arn = principal_arn.filter(|s| !s.is_empty()).map(str::to_string);
    TrustedPrincipal::iam(account_id, arn)
}

/// Extracts the `name -> stringValue` map from an SQS record's
/// `messageAttributes`, ignoring non-string attributes. Missing attributes
/// yield an empty map, which [`TrustedPrincipal::from_sqs_attributes`] then
/// rejects as a missing envelope.
pub fn sqs_attribute_map(record: &Value) -> HashMap<String, String> {
    record
        .get("messageAttributes")
        .and_then(Value::as_object)
        .map(|obj| {
            obj.iter()
                .filter_map(|(name, attr)| {
                    attr.get("stringValue")
                        .and_then(Value::as_str)
                        .map(|s| (name.clone(), s.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

impl TrustedPrincipal {
    /// Reconstructs a principal from the SQS message attributes written by the
    /// domain and subscription mapping templates.
    ///
    /// The command body is intentionally not read here; identity comes only
    /// from the envelope attributes so caller-supplied body fields are inert.
    pub fn from_sqs_attributes(attrs: &HashMap<String, String>) -> CtxResult<TrustedPrincipal> {
        check_version(attrs.get(attr::VERSION))?;
        match required(attrs, attr::MECHANISM)? {
            "oauth" => Ok(build_oauth(
                required(attrs, attr::PRINCIPAL_ID)?,
                required(attrs, attr::ISSUER)?,
                required(attrs, attr::CLIENT_ID)?,
                parse_global_roles(attrs.get(attr::GLOBAL_ROLES).map(String::as_str))?,
                parse_team_groups(attrs.get(attr::TEAM_GROUPS).map(String::as_str))?,
            )),
            "iam" => Ok(build_iam(
                required(attrs, attr::ACCOUNT_ID)?,
                attrs.get(attr::PRINCIPAL_ARN).map(String::as_str),
            )),
            other => Err(PrincipalContextError::UnknownMechanism(other.to_string())),
        }
    }

    /// Reconstructs a principal from the API Gateway proxy `requestContext`
    /// seen by Lambda proxy handlers.
    ///
    /// OAuth surfaces carry the authorizer context under `authorizer`; IAM
    /// surfaces carry the SigV4 caller under `identity`. Anything else is a
    /// fail-closed [`PrincipalContextError::MissingContext`].
    pub fn from_proxy_request_context(ctx: &Value) -> CtxResult<TrustedPrincipal> {
        if let Some(authorizer) = ctx.get("authorizer") {
            let mechanism = authorizer
                .get("authenticationMechanism")
                .and_then(Value::as_str)
                .ok_or(PrincipalContextError::MissingField(
                    "authenticationMechanism",
                ))?;
            if mechanism != "oauth" {
                return Err(PrincipalContextError::UnknownMechanism(
                    mechanism.to_string(),
                ));
            }
            let get = |k: &'static str| -> CtxResult<&str> {
                authorizer
                    .get(k)
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .ok_or(PrincipalContextError::MissingField(k))
            };
            return Ok(build_oauth(
                get("principalId")?,
                get("issuer")?,
                get("clientId")?,
                parse_global_roles(authorizer.get("globalRoles").and_then(Value::as_str))?,
                parse_team_groups(authorizer.get("teamGroups").and_then(Value::as_str))?,
            ));
        }

        if let Some(identity) = ctx.get("identity") {
            let account_id = identity
                .get("accountId")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .ok_or(PrincipalContextError::MissingField("accountId"))?;
            let principal_arn = identity.get("userArn").and_then(Value::as_str);
            return Ok(build_iam(account_id, principal_arn));
        }

        Err(PrincipalContextError::MissingContext)
    }

    /// Serializes the principal into the SQS message-attribute envelope that
    /// [`TrustedPrincipal::from_sqs_attributes`] parses — the same attributes
    /// the API Gateway mapping templates write. The publish admission Lambda
    /// uses this to rebuild the trusted envelope when it takes over enqueueing
    /// from the direct SQS integration.
    pub fn to_sqs_attributes(&self) -> HashMap<String, String> {
        let mut attrs = HashMap::new();
        attrs.insert(
            attr::VERSION.to_string(),
            TRUSTED_PRINCIPAL_VERSION.to_string(),
        );
        match self {
            TrustedPrincipal::OAuth(p) => {
                attrs.insert(attr::MECHANISM.to_string(), "oauth".to_string());
                attrs.insert(attr::PRINCIPAL_ID.to_string(), p.subject.clone());
                attrs.insert(attr::ISSUER.to_string(), p.issuer.clone());
                attrs.insert(attr::CLIENT_ID.to_string(), p.client_id.clone());
                attrs.insert(
                    attr::GLOBAL_ROLES.to_string(),
                    serde_json::to_string(&p.global_roles).unwrap_or_else(|_| "[]".to_string()),
                );
                attrs.insert(
                    attr::TEAM_GROUPS.to_string(),
                    serde_json::to_string(&p.team_groups).unwrap_or_else(|_| "[]".to_string()),
                );
            }
            TrustedPrincipal::Iam(p) => {
                attrs.insert(attr::MECHANISM.to_string(), "iam".to_string());
                attrs.insert(attr::ACCOUNT_ID.to_string(), p.account_id.clone());
                if let Some(arn) = &p.principal_arn {
                    attrs.insert(attr::PRINCIPAL_ARN.to_string(), arn.clone());
                }
            }
        }
        attrs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principal::GlobalRole;

    #[test]
    fn oauth_principal_round_trips_through_sqs_attributes() {
        let principal = TrustedPrincipal::oauth(
            "sub-123",
            "https://cognito-idp.us-east-1.amazonaws.com/us-east-1_TestPool",
            "client-1",
            vec![GlobalRole::PlatformAdmin],
            vec!["team-orders".to_string()],
        );
        let attrs = principal.to_sqs_attributes();
        let parsed = TrustedPrincipal::from_sqs_attributes(&attrs).unwrap();
        assert_eq!(parsed, principal);
    }

    #[test]
    fn iam_principal_round_trips_through_sqs_attributes() {
        let principal = TrustedPrincipal::iam(
            "111122223333",
            Some("arn:aws:iam::111122223333:role/publisher".to_string()),
        );
        let attrs = principal.to_sqs_attributes();
        let parsed = TrustedPrincipal::from_sqs_attributes(&attrs).unwrap();
        assert_eq!(parsed, principal);
    }

    #[test]
    fn iam_principal_without_arn_round_trips() {
        let principal = TrustedPrincipal::iam("111122223333", None);
        let attrs = principal.to_sqs_attributes();
        assert!(!attrs.contains_key(attr::PRINCIPAL_ARN));
        let parsed = TrustedPrincipal::from_sqs_attributes(&attrs).unwrap();
        assert_eq!(parsed, principal);
    }
}
