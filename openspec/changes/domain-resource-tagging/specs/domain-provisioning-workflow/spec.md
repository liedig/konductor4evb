## ADDED Requirements

### Requirement: Tagging at provisioning

Provisioning SHALL tag every resource it creates with the domain's effective tag set. Per-environment resources (platform event bus, app event bus, schema registry, subscription DLQ, integration DLQ, dashboard, alarms) SHALL carry `konductor:managed-by`, `konductor:domain`, `konductor:environment`, and `konductor:namespace`, plus `cost-center` when the domain record carries one. The domain-level namespace SSM parameter SHALL carry the same set minus `konductor:environment`. Tags SHALL be supplied at resource creation wherever the service supports it; tag application SHALL be part of the saga's tracked steps so a tagging failure fails (and compensates) the environment exactly like any other resource failure.

#### Scenario: Provisioned bus carries the effective tags

- **WHEN** a domain with `cost-center=cc-1234` is provisioned into `dev`
- **THEN** `acme-dev-<domain>-event-bus` in both accounts SHALL be tagged `konductor:managed-by=konductor`, `konductor:domain=<domain>`, `konductor:environment=dev`, `konductor:namespace=<org>-<domain>`, and `cost-center=cc-1234`

#### Scenario: Namespace parameter tagged without environment

- **WHEN** the domain-level namespace parameter is created
- **THEN** it SHALL carry `konductor:managed-by`, `konductor:domain`, and `konductor:namespace`, and SHALL NOT carry `konductor:environment`

#### Scenario: Tag failure fails the environment

- **WHEN** a tagging call fails during provisioning of an environment
- **THEN** that environment SHALL fail and compensate exactly as for any other resource-creation failure
