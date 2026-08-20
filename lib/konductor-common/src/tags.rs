use crate::namespace::NamespaceResolver;

/// The platform's reserved tag prefix; custom tags never use it.
pub const TAG_PREFIX: &str = "konductor:";

pub const TAG_MANAGED_BY: &str = "konductor:managed-by";
pub const TAG_DOMAIN: &str = "konductor:domain";
pub const TAG_ENVIRONMENT: &str = "konductor:environment";
pub const TAG_NAMESPACE: &str = "konductor:namespace";
pub const TAG_COST_CENTER: &str = "cost-center";

/// The one governed custom tag key, settable at registration.
pub const COST_CENTER_FIELD: &str = "costCenter";

/// The effective tag set for a domain's resources: the standard platform set
/// plus `cost-center` when the registration carried one. `environment` is
/// present only for per-environment resources; domain-level resources (the
/// namespace parameter) pass `None`.
pub fn effective_tags(
    org: &str,
    domain: &str,
    environment: Option<&str>,
    cost_center: Option<&str>,
) -> Vec<(String, String)> {
    let resolver = NamespaceResolver::default_pattern();
    let mut tags = vec![
        (TAG_MANAGED_BY.to_string(), "konductor".to_string()),
        (TAG_DOMAIN.to_string(), domain.to_string()),
        (TAG_NAMESPACE.to_string(), resolver.namespace(org, domain)),
    ];
    if let Some(env) = environment {
        tags.push((TAG_ENVIRONMENT.to_string(), env.to_string()));
    }
    if let Some(cc) = cost_center.filter(|v| !v.is_empty()) {
        tags.push((TAG_COST_CENTER.to_string(), cc.to_string()));
    }
    tags
}

/// AWS tag-value legality for the governed custom tag: non-empty when present,
/// ≤256 chars, no control characters.
pub fn validate_cost_center(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("costCenter must not be empty".to_string());
    }
    if value.chars().count() > 256 {
        return Err("costCenter must be at most 256 characters".to_string());
    }
    if value.chars().any(|c| c.is_control()) {
        return Err("costCenter must not contain control characters".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_set_with_environment() {
        let tags = effective_tags("acme", "sales", Some("dev"), None);
        assert_eq!(
            tags,
            vec![
                (TAG_MANAGED_BY.to_string(), "konductor".to_string()),
                (TAG_DOMAIN.to_string(), "sales".to_string()),
                (TAG_NAMESPACE.to_string(), "acme-sales".to_string()),
                (TAG_ENVIRONMENT.to_string(), "dev".to_string()),
            ]
        );
    }

    #[test]
    fn domain_level_set_omits_environment() {
        let tags = effective_tags("acme", "sales", None, Some("cc-1234"));
        assert!(!tags.iter().any(|(k, _)| k == TAG_ENVIRONMENT));
        assert!(tags.contains(&(TAG_COST_CENTER.to_string(), "cc-1234".to_string())));
    }

    #[test]
    fn empty_cost_center_is_dropped() {
        let tags = effective_tags("acme", "sales", Some("prod"), Some(""));
        assert!(!tags.iter().any(|(k, _)| k == TAG_COST_CENTER));
    }

    #[test]
    fn cost_center_validation() {
        assert!(validate_cost_center("cc-1234").is_ok());
        assert!(validate_cost_center("").is_err());
        assert!(validate_cost_center(&"x".repeat(257)).is_err());
        assert!(validate_cost_center("bad\nvalue").is_err());
    }
}
