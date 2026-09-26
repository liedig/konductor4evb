//! Per-environment account overrides on a domain record.
//!
//! A domain may pin individual platform environments to accounts other than
//! `ownerAccountId` via the sparse `environmentAccounts` map. The map is
//! fixed at registration; every consumer resolves the effective account as
//! `map[environment] ?? ownerAccountId`, so a missing key — or a missing
//! map — yields byte-identical behavior to a mapless registration.

use std::collections::HashMap;

/// Twelve ASCII digits, the AWS account ID shape the trust stack enforces.
pub fn is_valid_account_id(value: &str) -> bool {
    value.len() == 12 && value.bytes().all(|b| b.is_ascii_digit())
}

/// Validates a caller-supplied sparse map against the platform environment
/// list. Failures name the offending key or value so the rejection is
/// actionable without reading logs.
pub fn validate_environment_accounts(
    overrides: &HashMap<String, String>,
    environments: &[String],
) -> Result<(), String> {
    for (environment, account) in overrides {
        if !environments.contains(environment) {
            return Err(format!(
                "environmentAccounts key '{environment}' is not a configured platform environment"
            ));
        }
        if !is_valid_account_id(account) {
            return Err(format!(
                "environmentAccounts['{environment}'] must be a 12-digit AWS account ID, got '{account}'"
            ));
        }
    }
    Ok(())
}

/// The account one environment resolves to: the override when present,
/// otherwise the domain's default account.
pub fn resolve_account<'a>(
    overrides: Option<&'a HashMap<String, String>>,
    owner_account_id: &'a str,
    environment: &str,
) -> &'a str {
    overrides
        .and_then(|map| map.get(environment))
        .map(String::as_str)
        .unwrap_or(owner_account_id)
}

/// The complete environment → account map for the domain: every platform
/// environment appears, with overrides applied. Approval, provisioning, and
/// retry consume this resolved shape rather than re-implementing the
/// fallback.
pub fn effective_accounts(
    overrides: Option<&HashMap<String, String>>,
    owner_account_id: &str,
    environments: &[String],
) -> HashMap<String, String> {
    environments
        .iter()
        .map(|environment| {
            (
                environment.clone(),
                resolve_account(overrides, owner_account_id, environment).to_string(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platform_environments() -> Vec<String> {
        vec!["dev".to_string(), "staging".to_string(), "prod".to_string()]
    }

    #[test]
    fn account_id_accepts_twelve_digits() {
        assert!(is_valid_account_id("111122223333"));
        assert!(is_valid_account_id("000000000000"));
    }

    #[test]
    fn account_id_rejects_malformed_values() {
        for value in ["", "123", "1111222233334", "11112222333a", " 111122223333"] {
            assert!(!is_valid_account_id(value), "'{value}' must be rejected");
        }
    }

    #[test]
    fn resolve_falls_back_to_the_owner_account_without_overrides() {
        let environments = platform_environments();
        for environment in &environments {
            assert_eq!(
                resolve_account(None, "111122223333", environment),
                "111122223333"
            );
        }
        let empty = HashMap::new();
        assert_eq!(
            resolve_account(Some(&empty), "111122223333", "prod"),
            "111122223333",
            "an empty map behaves like an absent map"
        );
    }

    #[test]
    fn resolve_applies_sparse_overrides_per_environment() {
        let overrides = HashMap::from([("prod".to_string(), "999988887777".to_string())]);
        assert_eq!(
            resolve_account(Some(&overrides), "111122223333", "prod"),
            "999988887777"
        );
        assert_eq!(
            resolve_account(Some(&overrides), "111122223333", "dev"),
            "111122223333",
            "environments without an override keep the default account"
        );
    }

    #[test]
    fn duplicate_accounts_across_environments_are_legal() {
        let overrides = HashMap::from([
            ("staging".to_string(), "999988887777".to_string()),
            ("prod".to_string(), "999988887777".to_string()),
        ]);
        assert!(validate_environment_accounts(&overrides, &platform_environments()).is_ok());
        let effective =
            effective_accounts(Some(&overrides), "111122223333", &platform_environments());
        assert_eq!(effective["staging"], "999988887777");
        assert_eq!(effective["prod"], "999988887777");
        assert_eq!(effective["dev"], "111122223333");
    }

    #[test]
    fn validation_rejects_keys_outside_the_platform_environment_list() {
        let overrides = HashMap::from([("qa".to_string(), "999988887777".to_string())]);
        let err = validate_environment_accounts(&overrides, &platform_environments())
            .expect_err("unknown key must be rejected");
        assert!(err.contains("'qa'"), "error names the offending key: {err}");
    }

    #[test]
    fn validation_rejects_malformed_account_values() {
        let overrides = HashMap::from([("prod".to_string(), "not-an-account".to_string())]);
        let err = validate_environment_accounts(&overrides, &platform_environments())
            .expect_err("malformed value must be rejected");
        assert!(
            err.contains("not-an-account") && err.contains("prod"),
            "error names the offending value and its key: {err}"
        );
    }

    #[test]
    fn validation_accepts_an_empty_map() {
        assert!(validate_environment_accounts(&HashMap::new(), &platform_environments()).is_ok());
    }

    #[test]
    fn effective_map_covers_every_platform_environment() {
        let effective = effective_accounts(None, "111122223333", &platform_environments());
        assert_eq!(effective.len(), 3);
        assert!(
            effective.values().all(|account| account == "111122223333"),
            "a mapless domain resolves every environment to the default account"
        );
    }
}
