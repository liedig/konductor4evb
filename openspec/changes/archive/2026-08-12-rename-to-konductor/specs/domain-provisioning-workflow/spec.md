# Delta: domain-provisioning-workflow (rename-to-konductor)

## MODIFIED Requirements

### Requirement: Provisioning workflow lifecycle events

The provisioning workflow SHALL emit lifecycle events to the Konductor event bus for provisioning outcomes only. No intermediate events SHALL be emitted (neither per-step within an env nor per-env completion). The outer workflow SHALL be the sole emitter of `DomainProvisioningSucceeded` and `DomainProvisioningFailed`. The event source SHALL be `konductor.domain-management`. Event payloads SHALL identify the domain by `domainName` and SHALL NOT include a `domainId` field.

#### Scenario: Provisioning succeeded event

- **WHEN** the outer provisioning workflow completes all environments successfully
- **THEN** it SHALL emit a `DomainProvisioningSucceeded` event with source `konductor.domain-management`, `domainName`, `workflowExecutionId`, `completedAt`, `environments`, `resourceArns` (map keyed by environment), `namespaceParameter` (SSM parameter path), and `isNewAccount` flag

#### Scenario: Provisioning failed event

- **WHEN** the outer provisioning workflow fails at any environment
- **THEN** it SHALL emit a `DomainProvisioningFailed` event with source `konductor.domain-management`, `domainName`, `workflowExecutionId`, `failedAt`, `failedEnvironment` (the first environment observed to fail), `failedStep` (step within that environment), `errorCode`, `errorMessage`, and `compensationErrors` (aggregate across inner and outer compensation)

### Requirement: Provisioning uses platform-layer names for Konductor-internal resources

The provisioning workflow SHALL reference Konductor platform resources (the Konductor event bus, execution roles, log groups, DynamoDB tables) using the fixed `{org}-konductor-*` pattern with no environment segment. Domain-resource names (per-env buses, registries, DLQs, dashboards, alarms) are unaffected by this change — they continue to follow the `{org}-{env}-{domain}-*` pattern established by `multi-env-domain-provisioning`.

#### Scenario: Provisioning emits events on the Konductor platform bus

- **WHEN** the provisioning workflow emits `DomainProvisioningSucceeded` or `DomainProvisioningFailed` events
- **THEN** the target event bus SHALL be `{org}-konductor-bus`

#### Scenario: Provisioning workflow definition substitutions exclude `Environment`

- **WHEN** the provisioning workflow is built from its ASL definition
- **THEN** the `DefinitionSubstitutions` block SHALL NOT include an `Environment` key
- **AND THEN** no substitution in the state machine SHALL reference `${Environment}` — env is sourced from the domain record per Map iteration, not from a deploy-time SAM substitution
