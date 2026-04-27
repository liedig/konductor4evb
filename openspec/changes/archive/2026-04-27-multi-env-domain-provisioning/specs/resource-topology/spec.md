## MODIFIED Requirements

### Requirement: Platform account domain resources

When a domain is provisioned, EventBridge Nexus SHALL create the following resources in the platform account **for each environment** in the domain's `environments` list: a domain event bus, a PutEvent policy restricting publishing to the domain's owning application account, a schema registry with access policies, a subscription rule DLQ (SQS queue), and CloudWatch observability resources (dashboard, alarms, metrics configuration).

Each environment SHALL produce a disjoint set of resources identifiable by its env segment in the name. A single domain registration request with `environments: ["dev", "staging", "prod"]` SHALL produce three full sets of resources in the platform account.

#### Scenario: Platform resources created per environment

- **WHEN** the provisioning workflow runs for a domain with `environments: ["dev", "staging", "prod"]`
- **THEN** the platform account SHALL contain three domain event buses (`{org}-dev-{domain}-domain-bus`, `{org}-staging-{domain}-domain-bus`, `{org}-prod-{domain}-domain-bus`), three schema registries, three subscription DLQs, and three dashboards/alarm sets, one per environment

#### Scenario: Domain event bus created in platform account

- **WHEN** the provisioning workflow creates the domain event bus in the platform account for a given environment
- **THEN** the bus SHALL be named according to the `{org}-{env}-{domain}-domain-bus` convention using that environment's value
- **AND THEN** the bus SHALL exist in the platform account where EventBridge Nexus operates

#### Scenario: PutEvent policy restricts publishing to domain owner

- **WHEN** the domain event bus is created in the platform account
- **THEN** Nexus SHALL attach a resource policy to the bus that permits `events:PutEvents` only from the domain's owning application account
- **AND THEN** the policy SHALL be identical across environments — all environments of the domain share the same owner account

### Requirement: Application account domain resources

When a domain is provisioned, EventBridge Nexus SHALL create the following resources in the domain's owning application account **for each environment** in the domain's `environments` list: a domain event bus (where the app receives routed events) and an integration rule DLQ (SQS queue). All environments of a given domain SHALL be provisioned into the same application account (the `ownerAccountId` on the domain record).

#### Scenario: Application resources created per environment

- **WHEN** the provisioning workflow runs for a domain with `environments: ["dev", "staging", "prod"]` and `ownerAccountId: "111122223333"`
- **THEN** account `111122223333` SHALL contain three domain event buses and three integration DLQs, one per environment, all named according to `{org}-{env}-{domain}-*`
- **AND THEN** account `111122223333` SHALL have exactly one trust role (`{org}-nexus-trust-role`) serving all three environments

#### Scenario: Domain event bus created in application account

- **WHEN** the provisioning workflow runs for a given environment
- **THEN** Nexus SHALL create a domain event bus in the owning application account, named `{org}-{env}-{domain}-domain-bus`
- **AND THEN** this bus SHALL be the target for subscription rules that route events from the platform domain bus to the application account

#### Scenario: Integration rule DLQ created in application account

- **WHEN** the provisioning workflow runs for a given environment
- **THEN** Nexus SHALL create an SQS queue in the application account named `{org}-{env}-{domain}-integration-dlq` to serve as the dead-letter queue for integration rules
- **AND THEN** failed deliveries from integration rules SHALL be routed to this DLQ
