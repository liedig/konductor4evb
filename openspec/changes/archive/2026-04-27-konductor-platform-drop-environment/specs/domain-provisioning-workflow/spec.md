## MODIFIED Requirements

### Requirement: Provisioning uses platform-layer names for Konductor-internal resources

The provisioning workflow SHALL reference Konductor platform resources (the Konductor event bus, execution roles, log groups, DynamoDB tables) using the fixed `{org}-konductor-*` pattern with no environment segment. Domain-resource names (per-env buses, registries, DLQs, dashboards, alarms) are unaffected by this change — they continue to follow the `{org}-{env}-{domain}-*` pattern established by `multi-env-domain-provisioning`.

#### Scenario: Provisioning emits events on the Konductor platform bus

- **WHEN** the provisioning workflow emits `DomainProvisioningSucceeded` or `DomainProvisioningFailed` events
- **THEN** the target event bus SHALL be `{org}-konductor-bus`

#### Scenario: Provisioning workflow definition substitutions exclude `Environment`

- **WHEN** the provisioning workflow is built from its ASL definition
- **THEN** the `DefinitionSubstitutions` block SHALL NOT include an `Environment` key
- **AND THEN** no substitution in the state machine SHALL reference `${Environment}` — env is sourced from the domain record per Map iteration, not from a deploy-time SAM substitution
