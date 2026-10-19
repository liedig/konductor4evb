//! The platform's configured environment list. The list is fixed at setup
//! and its order is the schema promotion pipeline order: the first entry is
//! the authoring environment, and each later entry receives versions only by
//! promotion from the entry immediately before it.

use crate::api::validation_error;
use crate::error::ControlPlaneError;

/// Reads `ENVIRONMENTS` (comma-separated) into the configured
/// environment list, preserving order.
pub fn configured_environments() -> Result<Vec<String>, ControlPlaneError> {
    let raw = std::env::var("ENVIRONMENTS")
        .map_err(|_| ControlPlaneError::Schemas("ENVIRONMENTS not set".to_string()))?;
    parse_environments(&raw)
}

/// The first configured environment: the only stage that accepts schema
/// authoring.
pub fn authoring_environment() -> Result<String, ControlPlaneError> {
    authoring_environment_of(&configured_environments()?)
}

/// The environment immediately before `target` in the promotion pipeline.
/// `Ok(None)` means `target` is the authoring environment; an unknown stage
/// is a caller error.
pub fn promotion_source(target: &str) -> Result<Option<String>, ControlPlaneError> {
    promotion_source_in(&configured_environments()?, target)
}

/// The first entry of a parsed environment list.
pub fn authoring_environment_of(environments: &[String]) -> Result<String, ControlPlaneError> {
    environments
        .first()
        .cloned()
        .ok_or_else(|| ControlPlaneError::Schemas("ENVIRONMENTS holds no environments".to_string()))
}

/// The previous entry of `target` within a parsed environment list.
pub fn promotion_source_in(
    environments: &[String],
    target: &str,
) -> Result<Option<String>, ControlPlaneError> {
    match environments.iter().position(|entry| entry == target) {
        Some(0) => Ok(None),
        Some(index) => Ok(Some(environments[index - 1].clone())),
        None => Err(validation_error(format!(
            "unknown environment stage '{target}'; configured environments: {}",
            environments.join(", ")
        ))),
    }
}

fn parse_environments(raw: &str) -> Result<Vec<String>, ControlPlaneError> {
    let environments: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_string)
        .collect();
    if environments.is_empty() {
        return Err(ControlPlaneError::Schemas(
            "ENVIRONMENTS holds no environments".to_string(),
        ));
    }
    Ok(environments)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn environments() -> Vec<String> {
        parse_environments("dev,staging,prod").unwrap()
    }

    #[test]
    fn parses_comma_separated_list_in_order() {
        assert_eq!(environments(), vec!["dev", "staging", "prod"]);
    }

    #[test]
    fn empty_list_is_an_error() {
        assert!(parse_environments("").is_err());
        assert!(parse_environments(" , ").is_err());
    }

    #[test]
    fn first_entry_is_the_authoring_environment() {
        assert_eq!(authoring_environment_of(&environments()).unwrap(), "dev");
    }

    #[test]
    fn promotion_source_is_the_previous_entry() {
        assert_eq!(
            promotion_source_in(&environments(), "staging").unwrap(),
            Some("dev".to_string())
        );
        assert_eq!(
            promotion_source_in(&environments(), "prod").unwrap(),
            Some("staging".to_string())
        );
        assert_eq!(promotion_source_in(&environments(), "dev").unwrap(), None);
    }

    #[test]
    fn promotion_source_rejects_unknown_stage() {
        assert!(promotion_source_in(&environments(), "qa").is_err());
    }

    #[test]
    fn reads_the_environment_variable() {
        std::env::set_var("ENVIRONMENTS", "sandbox,qa,live");
        assert_eq!(authoring_environment().unwrap(), "sandbox");
    }
}
