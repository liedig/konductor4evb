pub struct NamespaceResolver {
    pattern: String,
}

impl NamespaceResolver {
    pub fn new(pattern: String) -> Self {
        Self { pattern }
    }

    pub fn default_pattern() -> Self {
        Self::new("{org}-{env}-{domain}".to_string())
    }

    pub fn resolve(&self, env: &str, org: &str, domain: &str) -> String {
        self.pattern
            .replace("{env}", env)
            .replace("{org}", org)
            .replace("{domain}", domain)
    }

    pub fn event_bus_name(&self, env: &str, org: &str, domain: &str) -> String {
        format!("{}-event-bus", self.resolve(env, org, domain))
    }

    pub fn schema_registry_name(&self, env: &str, org: &str, domain: &str) -> String {
        format!("{}-schema-registry", self.resolve(env, org, domain))
    }

    pub fn subscription_dlq_name(&self, env: &str, org: &str, domain: &str) -> String {
        format!("{}-subscription-dlq", self.resolve(env, org, domain))
    }

    pub fn integration_dlq_name(&self, env: &str, org: &str, domain: &str) -> String {
        format!("{}-integration-dlq", self.resolve(env, org, domain))
    }

    pub fn namespace(&self, org: &str, domain: &str) -> String {
        format!("{org}-{domain}")
    }

    pub fn namespace_parameter_path(&self, domain: &str) -> String {
        format!("/namespace/{domain}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pattern_resolves_correctly() {
        let resolver = NamespaceResolver::default_pattern();
        assert_eq!(
            resolver.resolve("prod", "acme", "billing"),
            "acme-prod-billing"
        );
    }

    #[test]
    fn event_bus_name_includes_suffix() {
        let resolver = NamespaceResolver::default_pattern();
        assert_eq!(
            resolver.event_bus_name("prod", "acme", "billing"),
            "acme-prod-billing-event-bus"
        );
    }

    #[test]
    fn custom_pattern_resolves_correctly() {
        let resolver = NamespaceResolver::new("{org}/{env}/{domain}".to_string());
        assert_eq!(
            resolver.resolve("prod", "acme", "billing"),
            "acme/prod/billing"
        );
    }

    #[test]
    fn namespace_is_environment_independent() {
        let resolver = NamespaceResolver::default_pattern();
        assert_eq!(resolver.namespace("acme", "billing"), "acme-billing");
    }

    #[test]
    fn namespace_parameter_path_format() {
        let resolver = NamespaceResolver::default_pattern();
        assert_eq!(
            resolver.namespace_parameter_path("billing"),
            "/namespace/billing"
        );
    }
}
