## MODIFIED Requirements

### Requirement: Domain namespace convention

Domain resources SHALL be named according to a configurable pattern defaulting to `{org}-{env}-{domain}` (e.g., `acme-dev-billing`). Platform teams SHALL be able to configure this naming pattern. The convention SHALL support multiple environments (dev, staging, prod) to allow resource isolation per environment at the domain layer. The domain namespace (environment-independent identity) SHALL be `{org}-{domain}`.

Nexus platform resources — the control plane itself — SHALL NOT use this convention. Platform resources follow the fixed `{org}-nexus-*` pattern (see system-architecture spec) because Nexus is a singleton per organisation and has no environment concept at the platform layer.

#### Scenario: Default namespace applied to domain resources

- **WHEN** a domain is provisioned with org `acme`, domain name `billing`, and environment `dev`
- **THEN** the domain event bus in the platform account SHALL be named `acme-dev-billing-domain-bus` (or according to the configured convention)
- **AND THEN** the schema registry, DLQs, and other domain resources SHALL follow the same convention with appropriate suffixes

#### Scenario: Platform team configures custom convention

- **WHEN** a platform team specifies a custom namespace convention
- **THEN** all subsequently provisioned domain resources SHALL use the custom convention for naming
- **AND THEN** existing resources SHALL NOT be renamed (convention changes are forward-only)
- **AND THEN** custom conventions SHALL apply only to domain resources; Nexus platform resources SHALL remain named `{org}-nexus-*`

#### Scenario: Environment isolation between domain resources

- **WHEN** the same domain (e.g., `billing`) is provisioned across dev, staging, and prod environments
- **THEN** each environment SHALL have its own independent set of domain resources (buses, registries, DLQs)
- **AND THEN** there SHALL be no cross-environment event routing unless explicitly configured
- **AND THEN** the domain namespace `{org}-{domain}` SHALL be the same across all environments

### Requirement: Platform account domain resources

When a domain is provisioned, EventBridge Nexus SHALL create the following resources in the platform account for each target environment: a domain event bus, a PutEvent policy restricting publishing to the domain's owning application account, a schema registry with access policies, a subscription rule DLQ (SQS queue), and CloudWatch observability resources (dashboard, alarms, metrics configuration). These resources SHALL be env-scoped in their names (e.g., `{org}-{env}-{domain}-domain-bus`).

The Nexus platform itself SHALL separately own control-plane resources that are NOT per-domain and NOT env-scoped: the Nexus event bus (`{org}-nexus-bus`), the domains DynamoDB table, the Nexus API, command queues, execution roles, and supporting infrastructure. These platform resources are created once at installation, not per-domain.

#### Scenario: Domain event bus created in platform account

- **WHEN** the domain provisioning workflow creates the domain event bus in the platform account for a given environment
- **THEN** the bus SHALL be named according to the `{org}-{env}-{domain}-domain-bus` convention
- **AND THEN** the bus SHALL exist in the platform account where EventBridge Nexus operates

#### Scenario: PutEvent policy restricts publishing to domain owner

- **WHEN** the domain event bus is created in the platform account
- **THEN** Nexus SHALL attach a resource policy to the bus that permits `events:PutEvents` only from the domain's owning application account
- **AND THEN** no other application account SHALL be able to publish events to that domain bus

#### Scenario: Schema registry created per domain-environment

- **WHEN** the domain is provisioned for a given environment
- **THEN** Nexus SHALL create a schema registry in the platform account named `{org}-{env}-{domain}-schema-registry`
- **AND THEN** the registry SHALL have access policies granting write access to the domain owner and read access to authorised consumers

#### Scenario: Subscription rule DLQ created in platform account

- **WHEN** the domain is provisioned for a given environment
- **THEN** Nexus SHALL create an SQS queue in the platform account named `{org}-{env}-{domain}-subscription-dlq` to serve as the dead-letter queue for subscription rules on the domain event bus
- **AND THEN** failed deliveries from subscription rules (domain bus to subscriber bus) SHALL be routed to this DLQ

#### Scenario: Observability resources created per domain-environment

- **WHEN** the domain is provisioned for a given environment
- **THEN** Nexus SHALL create a CloudWatch dashboard, alarms, and metrics configuration for the domain event bus in the platform account
- **AND THEN** each environment SHALL have its own dashboard and alarm set

#### Scenario: Platform control-plane resources are singletons

- **WHEN** the Nexus platform stack is deployed
- **THEN** the Nexus event bus, domains table, Nexus API, command queues, and platform IAM roles SHALL be created once, named `{org}-nexus-*`, and SHALL NOT be duplicated per environment or per domain

### Requirement: Application account domain resources

When a domain is provisioned, EventBridge Nexus SHALL create the following resources in the domain's owning application account for each target environment: a domain event bus (where the app receives routed events) and an integration rule DLQ (SQS queue). These resources SHALL be env-scoped in their names.

#### Scenario: Domain event bus created in application account

- **WHEN** the domain provisioning workflow runs for a given environment
- **THEN** Nexus SHALL create a domain event bus in the owning application account, named `{org}-{env}-{domain}-domain-bus`
- **AND THEN** this bus SHALL be the target for subscription rules that route events from the platform domain bus to the application account

#### Scenario: Integration rule DLQ created in application account

- **WHEN** the domain provisioning workflow runs for a given environment
- **THEN** Nexus SHALL create an SQS queue in the application account named `{org}-{env}-{domain}-integration-dlq` to serve as the dead-letter queue for integration rules
- **AND THEN** failed deliveries from integration rules SHALL be routed to this DLQ
