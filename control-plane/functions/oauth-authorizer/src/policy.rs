use std::collections::HashMap;

use serde::Serialize;
use serde_json::{json, Value};

use nexus_common::principal::{GlobalRole, TrustedPrincipal};

/// Coarse route gate for the OAuth surface.
///
/// `platform-admin` may access every route. `platform-auditor` may access only
/// read (GET) routes and cannot mutate or approve. A principal with no global
/// role is denied (fail closed). Resource-level authorization is a downstream
/// responsibility and is intentionally not evaluated here.
pub fn route_allowed(http_method: &str, roles: &[GlobalRole]) -> bool {
    if roles.contains(&GlobalRole::PlatformAdmin) {
        return true;
    }
    http_method.eq_ignore_ascii_case("GET") && roles.contains(&GlobalRole::PlatformAuditor)
}

/// The effect of an execute-api policy statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Allow,
    Deny,
}

impl Effect {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "Allow",
            Self::Deny => "Deny",
        }
    }
}

/// The REQUEST authorizer result returned to API Gateway: a route-scoped
/// execute-api policy plus compact scalar trusted-principal context. Context
/// values are strings (list fields are JSON-encoded); no entitlements, request
/// body, or token material is included.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizerOutput {
    pub principal_id: String,
    pub policy_document: Value,
    pub context: HashMap<String, String>,
}

/// Builds the authorizer result for `principal` on the route identified by
/// `method_arn`. `context_usage` never carries the raw token.
pub fn build_output(
    principal: &TrustedPrincipal,
    method_arn: &str,
    effect: Effect,
) -> AuthorizerOutput {
    let policy_document = json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Action": "execute-api:Invoke",
            "Effect": effect.as_str(),
            "Resource": method_arn,
        }],
    });

    let mut context = HashMap::new();
    context.insert("authenticationMechanism".to_string(), "oauth".to_string());
    context.insert(
        "principalId".to_string(),
        principal.principal_id().to_string(),
    );
    context.insert(
        "globalRoles".to_string(),
        serde_json::to_string(&principal.global_roles()).unwrap_or_else(|_| "[]".to_string()),
    );
    context.insert(
        "teamGroups".to_string(),
        serde_json::to_string(&principal.team_groups()).unwrap_or_else(|_| "[]".to_string()),
    );
    if let TrustedPrincipal::OAuth(oauth) = principal {
        context.insert("issuer".to_string(), oauth.issuer.clone());
        context.insert("clientId".to_string(), oauth.client_id.clone());
    }

    AuthorizerOutput {
        principal_id: principal.principal_id().to_string(),
        policy_document,
        context,
    }
}

/// Extracts the HTTP method from an execute-api `methodArn`
/// (`arn:...:execute-api:.../{stage}/{METHOD}/{resource}`).
pub fn http_method_from_arn(method_arn: &str) -> Option<&str> {
    method_arn.split('/').nth(2).filter(|m| !m.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_common::principal::GlobalRole;

    fn principal(roles: Vec<GlobalRole>) -> TrustedPrincipal {
        TrustedPrincipal::oauth(
            "sub-1",
            "https://issuer",
            "client",
            roles,
            vec!["team-a".to_string()],
        )
    }

    #[test]
    fn admin_can_access_any_method() {
        let roles = vec![GlobalRole::PlatformAdmin];
        assert!(route_allowed("GET", &roles));
        assert!(route_allowed("POST", &roles));
        assert!(route_allowed("DELETE", &roles));
    }

    #[test]
    fn auditor_can_read_but_not_mutate() {
        let roles = vec![GlobalRole::PlatformAuditor];
        assert!(route_allowed("GET", &roles));
        assert!(route_allowed("get", &roles));
        assert!(!route_allowed("POST", &roles));
        assert!(!route_allowed("PUT", &roles));
        assert!(!route_allowed("DELETE", &roles));
    }

    #[test]
    fn no_role_is_denied() {
        assert!(!route_allowed("GET", &[]));
        assert!(!route_allowed("POST", &[]));
    }

    #[test]
    fn policy_is_scoped_to_the_invoked_route() {
        let out = build_output(
            &principal(vec![GlobalRole::PlatformAdmin]),
            "arn:aws:execute-api:r:acct:api/v1/POST/domains/registrations/decide",
            Effect::Allow,
        );
        assert_eq!(
            out.policy_document["Statement"][0]["Resource"],
            "arn:aws:execute-api:r:acct:api/v1/POST/domains/registrations/decide"
        );
        assert_eq!(out.policy_document["Statement"][0]["Effect"], "Allow");
    }

    #[test]
    fn context_is_compact_scalars_without_entitlements() {
        let out = build_output(
            &principal(vec![GlobalRole::PlatformAdmin]),
            "arn:aws:execute-api:r:acct:api/v1/GET/domains",
            Effect::Allow,
        );
        assert_eq!(out.context["authenticationMechanism"], "oauth");
        assert_eq!(out.context["principalId"], "sub-1");
        assert_eq!(out.context["globalRoles"], "[\"platform-admin\"]");
        assert_eq!(out.context["teamGroups"], "[\"team-a\"]");
        assert!(!out.context.contains_key("entitlements"));
        assert!(!out.context.contains_key("Authorization"));
    }

    #[test]
    fn parses_http_method_from_method_arn() {
        assert_eq!(
            http_method_from_arn("arn:aws:execute-api:r:acct:api/v1/POST/domains"),
            Some("POST")
        );
        assert_eq!(
            http_method_from_arn("arn:aws:execute-api:r:acct:api/v1/GET"),
            Some("GET")
        );
    }
}
