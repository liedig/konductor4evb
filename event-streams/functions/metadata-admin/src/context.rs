//! Trusted administration context: the deployment-configured organisation,
//! region, and environment list the preview/readiness routes resolve
//! against. Read from function environment variables in production and
//! injected directly in tests, so no handler path reads process state.

use konductor_common::error::ControlPlaneError;
use konductor_common::metadata::generation::GenerationContext;

/// The deployment-time configuration the main entrypoint reads from the
/// function environment and hands to the dispatch routes.
pub struct AdminConfig {
    pub region: String,
    pub organization: String,
    pub environments: Vec<String>,
}

impl AdminConfig {
    pub fn from_env() -> Result<Self, String> {
        let region = std::env::var("AWS_REGION").map_err(|_| "AWS_REGION not set")?;
        let organization = std::env::var("ORGANIZATION").map_err(|_| "ORGANIZATION not set")?;
        let raw = std::env::var("ENVIRONMENTS").map_err(|_| "ENVIRONMENTS not set")?;
        let environments = raw
            .split(',')
            .map(str::trim)
            .filter(|env| !env.is_empty())
            .map(String::from)
            .collect::<Vec<_>>();
        Ok(AdminConfig {
            region,
            organization,
            environments,
        })
    }

    /// Environment names are validated against deployment configuration,
    /// never accepted as free user text.
    pub fn is_configured_environment(&self, environment: &str) -> bool {
        self.environments.iter().any(|e| e == environment)
    }

    /// Builds the trusted generation context for one environment. Missing
    /// or empty sources are configuration unavailability (503), not a
    /// caller error.
    pub fn generation_context(
        &self,
        environment: &str,
    ) -> Result<GenerationContext, ControlPlaneError> {
        let unavailable = |source: &str| ControlPlaneError::MetadataConfigurationUnavailable {
            reason: format!("trusted {source} context is unavailable"),
        };
        if self.region.trim().is_empty() {
            return Err(unavailable("processing region"));
        }
        if self.organization.trim().is_empty() {
            return Err(unavailable("organisation"));
        }
        GenerationContext::new(
            chrono::Utc::now(),
            self.region.trim(),
            self.organization.trim(),
            environment,
        )
        .map_err(|e| ControlPlaneError::MetadataConfigurationUnavailable {
            reason: e.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AdminConfig {
        AdminConfig {
            region: "us-east-1".to_string(),
            organization: "acme".to_string(),
            environments: vec!["dev".to_string(), "prod".to_string()],
        }
    }

    #[test]
    fn environment_names_are_validated_against_configuration() {
        let config = config();
        assert!(config.is_configured_environment("dev"));
        assert!(!config.is_configured_environment("staging"));
    }

    #[test]
    fn empty_context_sources_are_unavailability() {
        let mut config = config();
        config.region = "  ".to_string();
        let error = config.generation_context("dev").unwrap_err();
        assert!(error
            .to_string()
            .contains("processing region context is unavailable"));
    }
}
