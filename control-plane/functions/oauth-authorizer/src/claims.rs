use serde::Deserialize;

use crate::error::AuthorizerError;
use konductor_common::principal::GlobalRole;

/// The standard Cognito access-token claims Nexus consumes. Signature,
/// expiration, and issuer are verified by the JWT layer; `token_use` and
/// `client_id` are asserted here. ID tokens are never accepted.
#[derive(Debug, Deserialize)]
pub struct AccessTokenClaims {
    /// Immutable Cognito subject; the OAuth `principalId`.
    pub sub: String,
    /// User-pool issuer.
    pub iss: String,
    /// Must be `access`.
    pub token_use: String,
    /// The OAuth app client the token was issued for.
    pub client_id: String,
    /// Validated Cognito group memberships.
    #[serde(rename = "cognito:groups", default)]
    pub groups: Vec<String>,
}

impl AccessTokenClaims {
    /// Asserts the token is an access token issued for the expected client.
    pub fn assert_for_client(&self, expected_client_id: &str) -> Result<(), AuthorizerError> {
        if self.token_use != "access" {
            return Err(AuthorizerError::WrongTokenUse);
        }
        if self.client_id != expected_client_id {
            return Err(AuthorizerError::WrongClient);
        }
        Ok(())
    }

    /// Splits validated `cognito:groups` into controlled global roles and
    /// opaque trusted team-group identifiers. Only exact controlled group names
    /// map to roles; everything else is preserved without permissions.
    pub fn roles_and_team_groups(&self) -> (Vec<GlobalRole>, Vec<String>) {
        let mut roles = Vec::new();
        let mut team_groups = Vec::new();
        for group in &self.groups {
            match GlobalRole::from_group_name(group) {
                Some(role) => roles.push(role),
                None => team_groups.push(group.clone()),
            }
        }
        (roles, team_groups)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(groups: &[&str]) -> AccessTokenClaims {
        AccessTokenClaims {
            sub: "sub".to_string(),
            iss: "https://issuer".to_string(),
            token_use: "access".to_string(),
            client_id: "client".to_string(),
            groups: groups.iter().map(|g| g.to_string()).collect(),
        }
    }

    #[test]
    fn maps_only_controlled_groups_to_roles() {
        let (roles, team_groups) =
            claims(&["platform-admin", "team-payments"]).roles_and_team_groups();
        assert_eq!(roles, vec![GlobalRole::PlatformAdmin]);
        assert_eq!(team_groups, vec!["team-payments".to_string()]);
    }

    #[test]
    fn preserves_unknown_groups_as_team_groups_without_roles() {
        let (roles, team_groups) = claims(&["team-a", "team-b"]).roles_and_team_groups();
        assert!(roles.is_empty());
        assert_eq!(
            team_groups,
            vec!["team-a".to_string(), "team-b".to_string()]
        );
    }

    #[test]
    fn rejects_id_token_use() {
        let mut c = claims(&[]);
        c.token_use = "id".to_string();
        assert!(matches!(
            c.assert_for_client("client"),
            Err(AuthorizerError::WrongTokenUse)
        ));
    }

    #[test]
    fn rejects_unexpected_client() {
        let c = claims(&[]);
        assert!(matches!(
            c.assert_for_client("other-client"),
            Err(AuthorizerError::WrongClient)
        ));
    }
}
