# Delta: resource-topology (rename-to-konductor)

## MODIFIED Requirements

### Requirement: Platform account domain resources

When a domain is provisioned, Konductor for Amazon EventBridge SHALL create the following resources in the platform account **for each environment** in the domain's `environments` list: a domain event bus, a PutEvent policy restricting publishing to the domain's owning domain account, a schema registry with access policies, a subscription rule DLQ (SQS queue), and CloudWatch observability resources (dashboard, alarms, metrics configuration).

Each environment SHALL produce a disjoint set of resources identifiable by its env segment in the name. A single domain registration request with `environments: ["dev", "staging", "prod"]` SHALL produce three full sets of resources in the platform account.

The Konductor platform itself SHALL separately own control-plane resources that are NOT per-domain and NOT env-scoped: the Konductor event bus (`{org}-konductor-bus`), the domains DynamoDB table, the Konductor API, command queues, execution roles, and supporting infrastructure. These platform resources are created once at installation, not per-domain.

#### Scenario: Platform resources created per environment

- **WHEN** the provisioning workflow runs for a domain with `environments: ["dev", "staging", "prod"]`
- **THEN** the platform account SHALL contain three domain event buses (`{org}-dev-{domain}-event-bus`, `{org}-staging-{domain}-event-bus`, `{org}-prod-{domain}-event-bus`), three schema registries, three subscription DLQs, and three dashboards/alarm sets, one per environment

#### Scenario: Domain event bus created in platform account

- **WHEN** the provisioning workflow creates the domain event bus in the platform account for a given environment
- **THEN** the bus SHALL be named according to the `{org}-{env}-{domain}-event-bus` convention using that environment's value
- **AND THEN** the bus SHALL exist in the platform account where Konductor for Amazon EventBridge operates

#### Scenario: PutEvent policy restricts publishing to domain owner

- **WHEN** the domain event bus is created in the platform account
- **THEN** Konductor SHALL attach a resource policy to the bus that permits `events:PutEvents` only from the domain's owning domain account
- **AND THEN** the policy SHALL be identical across environments — all environments of the domain share the same owner account

#### Scenario: Schema registry created per domain-environment

- **WHEN** the domain is provisioned for a given environment
- **THEN** Konductor SHALL create a schema registry in the platform account named `{org}-{env}-{domain}-schema-registry`
- **AND THEN** the registry SHALL have access policies granting write access to the domain owner and read access to authorised consumers

#### Scenario: Subscription rule DLQ created in platform account

- **WHEN** the domain is provisioned for a given environment
- **THEN** Konductor SHALL create an SQS queue in the platform account named `{org}-{env}-{domain}-subscription-dlq` to serve as the dead-letter queue for subscription rules on the domain event bus
- **AND THEN** failed deliveries from subscription rules (domain bus to subscriber bus) SHALL be routed to this DLQ

#### Scenario: Observability resources created per domain-environment

- **WHEN** the domain is provisioned for a given environment
- **THEN** Konductor SHALL create a CloudWatch dashboard, alarms, and metrics configuration for the domain event bus in the platform account
- **AND THEN** each environment SHALL have its own dashboard and alarm set

#### Scenario: Platform control-plane resources are singletons

- **WHEN** the Konductor platform stack is deployed
- **THEN** the Konductor event bus, domains table, Konductor API, command queues, and platform IAM roles SHALL be created once, named `{org}-konductor-*`, and SHALL NOT be duplicated per environment or per domain

### Requirement: Subscription rules on platform domain buses

Subscription rules SHALL live on the producer's platform-side domain event bus, one rule per subscription per environment, named `{consumerDomain}-{detailType}`. Rule patterns SHALL filter on the canonical envelope (`source`, `detail-type`, numeric `detail.metadata.schemaVersion` pinned per environment). Only Konductor SHALL manage these rules. The `konductor-event-streams` DynamoDB table SHALL be the authoritative registry of subscription topology; the rules are its materialization.

#### Scenario: Rule placement and naming

- **WHEN** the `checkout` domain subscribes to `prezzee-orders@OrderNotification`
- **THEN** each environment's platform bus `{org}-{env}-orders-event-bus` SHALL carry exactly one rule named `checkout-OrderNotification` for this subscription

#### Scenario: Topology queried from the table, not from AWS resources

- **WHEN** any process needs the set of active event streams (per producer, per consumer, or per schema version)
- **THEN** it SHALL query the `konductor-event-streams` table
- **AND THEN** it SHALL NOT enumerate EventBridge rules across buses to derive topology

### Requirement: Domain namespace convention

Domain resources SHALL be named according to a configurable pattern defaulting to `{org}-{env}-{domain}` (e.g., `acme-dev-billing`). Platform teams SHALL be able to configure this naming pattern. The convention SHALL support multiple environments (dev, staging, prod) to allow resource isolation per environment at the domain layer. The domain namespace (environment-independent identity) SHALL be `{org}-{domain}`.

Konductor platform resources — the control plane itself — SHALL NOT use this convention. Platform resources follow the fixed `{org}-konductor-*` pattern (see system-architecture spec) because Konductor is a singleton per organisation and has no environment concept at the platform layer.

#### Scenario: Default namespace applied to domain resources

- **WHEN** a domain is provisioned with org `acme`, domain name `billing`, and environment `dev`
- **THEN** the domain event bus in the platform account SHALL be named `acme-dev-billing-event-bus` (or according to the configured convention)
- **AND THEN** the schema registry, DLQs, and other domain resources SHALL follow the same convention with appropriate suffixes

#### Scenario: Platform team configures custom convention

- **WHEN** a platform team specifies a custom namespace convention
- **THEN** all subsequently provisioned domain resources SHALL use the custom convention for naming
- **AND THEN** existing resources SHALL NOT be renamed (convention changes are forward-only)
- **AND THEN** custom conventions SHALL apply only to domain resources; Konductor platform resources SHALL remain named `{org}-konductor-*`

#### Scenario: Environment isolation between domain resources

- **WHEN** the same domain (e.g., `billing`) is provisioned across dev, staging, and prod environments
- **THEN** each environment SHALL have its own independent set of domain resources (buses, registries, DLQs)
- **AND THEN** there SHALL be no cross-environment event routing unless explicitly configured
- **AND THEN** the domain namespace `{org}-{domain}` SHALL be the same across all environments
