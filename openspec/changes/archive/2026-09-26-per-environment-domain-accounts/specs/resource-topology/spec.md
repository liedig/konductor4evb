# Spec Delta

## MODIFIED Requirements

### Requirement: Domain account domain resources

When a domain is provisioned, Konductor for Amazon EventBridge SHALL create the following resources **for each environment** in the domain's `environments` list, in the account resolved for that environment (`ownerAccountId` by default, or the account named by an `environmentAccounts` override): a domain event bus (where the app receives routed events) and an integration rule DLQ (SQS queue).

#### Scenario: Application resources created per environment

- **WHEN** the provisioning workflow runs for a domain with `environments: ["dev", "staging", "prod"]` and `ownerAccountId: "111122223333"`
- **THEN** account `111122223333` SHALL contain three domain event buses and three integration DLQs, one per environment, all named according to `{org}-{env}-{domain}-*`
- **AND THEN** account `111122223333` SHALL have exactly one trust role (`{org}-konductor-trust-role`) serving all three environments

#### Scenario: Overridden environments land in the mapped account

- **WHEN** the provisioning workflow runs for a domain whose `prod` environment is overridden to account `444455556666` while `dev` and `staging` resolve to `ownerAccountId` `111122223333`
- **THEN** account `444455556666` SHALL contain the `prod` domain event bus and integration DLQ, and account `111122223333` SHALL contain the `dev` and `staging` sets
- **AND THEN** each distinct account used SHALL have exactly one trust role (`{org}-konductor-trust-role`)

#### Scenario: Domain event bus created in domain account

- **WHEN** the provisioning workflow runs for a given environment
- **THEN** Konductor SHALL create a domain event bus in the account resolved for that environment, named `{org}-{env}-{domain}-event-bus`
- **AND THEN** this bus SHALL be the target for subscription rules that route events from the platform domain bus to the domain account

#### Scenario: Integration rule DLQ created in domain account

- **WHEN** the provisioning workflow runs for a given environment
- **THEN** Konductor SHALL create an SQS queue in the account resolved for that environment, named `{org}-{env}-{domain}-integration-dlq` to serve as the dead-letter queue for integration rules
- **AND THEN** failed deliveries from integration rules SHALL be routed to this DLQ
