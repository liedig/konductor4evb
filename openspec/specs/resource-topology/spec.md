# resource-topology Specification

## Purpose

TBD - created by archiving change architecture-decisions. Update Purpose after archive.

## Requirements

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

### Requirement: Domain account domain resources

When a domain is provisioned, Konductor for Amazon EventBridge SHALL create the following resources in the domain's owning domain account **for each environment** in the domain's `environments` list: a domain event bus (where the app receives routed events) and an integration rule DLQ (SQS queue). All environments of a given domain SHALL be provisioned into the same domain account (the `ownerAccountId` on the domain record).

#### Scenario: Application resources created per environment

- **WHEN** the provisioning workflow runs for a domain with `environments: ["dev", "staging", "prod"]` and `ownerAccountId: "111122223333"`
- **THEN** account `111122223333` SHALL contain three domain event buses and three integration DLQs, one per environment, all named according to `{org}-{env}-{domain}-*`
- **AND THEN** account `111122223333` SHALL have exactly one trust role (`{org}-konductor-trust-role`) serving all three environments

#### Scenario: Domain event bus created in domain account

- **WHEN** the provisioning workflow runs for a given environment
- **THEN** Konductor SHALL create a domain event bus in the owning domain account, named `{org}-{env}-{domain}-event-bus`
- **AND THEN** this bus SHALL be the target for subscription rules that route events from the platform domain bus to the domain account

#### Scenario: Integration rule DLQ created in domain account

- **WHEN** the provisioning workflow runs for a given environment
- **THEN** Konductor SHALL create an SQS queue in the domain account named `{org}-{env}-{domain}-integration-dlq` to serve as the dead-letter queue for integration rules
- **AND THEN** failed deliveries from integration rules SHALL be routed to this DLQ

### Requirement: Subscription rules on platform domain buses

Subscription rules SHALL live on the producer's platform-side domain event bus, one rule per subscription per environment, named `{consumerDomain}-{detailType}`. Rule patterns SHALL filter on the canonical envelope (`source`, `detail-type`, numeric `detail.metadata.schemaVersion` pinned per environment). Only Konductor SHALL manage these rules. The `konductor-event-streams` DynamoDB table SHALL be the authoritative registry of subscription topology; the rules are its materialization.

#### Scenario: Rule placement and naming

- **WHEN** the `checkout` domain subscribes to `prezzee-orders@OrderNotification`
- **THEN** each environment's platform bus `{org}-{env}-orders-event-bus` SHALL carry exactly one rule named `checkout-OrderNotification` for this subscription

#### Scenario: Topology queried from the table, not from AWS resources

- **WHEN** any process needs the set of active event streams (per producer, per consumer, or per schema version)
- **THEN** it SHALL query the `konductor-event-streams` table
- **AND THEN** it SHALL NOT enumerate EventBridge rules across buses to derive topology

### Requirement: One domain bus per subscribing domain account

Each subscribing domain account SHALL receive events on a dedicated domain event bus per domain. There SHALL be a 1:1 mapping between domain and event bus per domain account. Multiple domains operating within a single domain account SHALL each have their own separate domain event bus in that account.

#### Scenario: Multiple domains in one domain account

- **WHEN** a domain account owns or subscribes to multiple domains (e.g., billing and sales)
- **THEN** each domain SHALL have its own dedicated event bus in that account
- **AND THEN** events for each domain SHALL be routed to the corresponding domain-specific bus

#### Scenario: Subscription routing targets domain account bus

- **WHEN** a subscription is created for a consumer in a different domain account
- **THEN** the subscription rule on the platform domain bus SHALL target the domain event bus in the subscriber's domain account
- **AND THEN** the delivery SHALL be authorized by the provision-time resource policy on the subscriber's bus permitting `events:PutEvents` from the platform account

### Requirement: Application team responsibility boundary

Application teams SHALL only interact with the domain event bus in their own account. They SHALL create integration rules on their own bus to process events (e.g., triggering Lambda functions, SQS queues, or other targets). Application teams SHALL NOT create or manage rules on the platform domain bus.

#### Scenario: Application team creates integration rules

- **WHEN** events are delivered to the domain event bus in the domain account
- **THEN** the application team SHALL create EventBridge rules on their own domain bus to route events to their consuming applications
- **AND THEN** the Konductor SHALL NOT manage or interfere with integration rules created by the application team

#### Scenario: Application team cannot access platform domain bus

- **WHEN** an application team attempts to create rules or modify the domain event bus in the platform account
- **THEN** IAM policies SHALL deny the operation
- **AND THEN** only the Konductor SHALL have permission to manage rules and policies on the platform domain bus

### Requirement: Event flow path

Events SHALL flow through a three-stage path: (1) the publishing application calls PutEvents on the domain event bus in the platform account, (2) subscription rules on the platform domain bus match and route events to the subscriber's domain event bus in their domain account, authorized by the provision-time resource policy permitting the platform account to PutEvents on the subscriber's bus, (3) the subscriber's integration rules on their own bus deliver events to their consuming applications.

#### Scenario: End-to-end event delivery

- **WHEN** a publishing application sends an event to the domain event bus in the platform account
- **THEN** the event SHALL be evaluated against subscription rules on that bus
- **AND THEN** matching subscription rules SHALL deliver the event to the target domain event bus in the subscriber's domain account under the platform-account resource policy on that bus
- **AND THEN** the subscriber's integration rules SHALL process the event on their own bus

#### Scenario: Subscription rule delivery failure

- **WHEN** a subscription rule on the platform domain bus fails to deliver an event to the subscriber's domain account bus
- **THEN** the event SHALL be sent to the subscription rule DLQ in the platform account
- **AND THEN** the DLQ message SHALL include error details (error code, error message, rule ARN, target ARN)

#### Scenario: Integration rule delivery failure

- **WHEN** an integration rule on the subscriber's domain bus fails to deliver an event to the consuming application
- **THEN** the event SHALL be sent to the integration rule DLQ in the domain account
- **AND THEN** the DLQ message SHALL include error details for troubleshooting

#### Scenario: Unmatched events are not delivered and not dead-lettered

- **WHEN** an event on the platform domain bus matches no subscription rule (e.g., its stamped schema version differs from every subscriber's pinned version)
- **THEN** the event SHALL NOT be delivered to any subscriber and SHALL NOT appear in any DLQ
- **AND THEN** detection of unexpectedly silent streams is an observability concern (zero-events alarming), not a delivery-path concern

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

### Requirement: Domain namespace identity

Each domain SHALL have a stable, environment-independent namespace of the form `{org}-{domain}` (e.g., `acme-billing`). The namespace SHALL be the domain's logical identity within the organisation. The namespace SHALL NOT include an environment component. The namespace SHALL be stored as an SSM Advanced Parameter at `/namespace/{domain}` with the namespace value as the parameter value.

#### Scenario: Namespace derived from organisation and domain

- **WHEN** a domain named `billing` is registered in organisation `acme`
- **THEN** the domain namespace SHALL be `acme-billing`
- **AND THEN** this namespace SHALL be the same regardless of which environment (dev, staging, prod) the domain is provisioned in

#### Scenario: Namespace stored as SSM parameter

- **WHEN** a domain is provisioned
- **THEN** an SSM Advanced parameter SHALL be created at `/namespace/{domainName}` with value `{org}-{domainName}`
- **AND THEN** the parameter SHALL be readable by all accounts in the AWS Organisation via RAM sharing

#### Scenario: Application team discovers domain namespaces

- **WHEN** an application team in any account within the AWS Organisation needs to discover available domains
- **THEN** they SHALL be able to list and read SSM parameters under `/namespace/` in their own account
- **AND THEN** the parameter values SHALL provide the namespace identifiers needed to construct resource names

### Requirement: RAM-shared namespace discovery

All domain namespace SSM parameters SHALL be shared with the AWS Organisation via a single AWS RAM resource share. Domain accounts SHALL have read-only access to namespace parameters without requiring cross-account IAM configuration.

#### Scenario: RAM share contains all namespace parameters

- **WHEN** multiple domains are provisioned (e.g., billing, orders, payments)
- **THEN** a single RAM resource share named `{org}-domain-namespaces` SHALL contain the SSM parameters for all domain namespaces
- **AND THEN** all accounts in the AWS Organisation SHALL be principals on the share

#### Scenario: New namespace automatically shared

- **WHEN** a new domain is provisioned and its namespace parameter is created
- **THEN** the parameter SHALL be automatically associated with the existing RAM resource share
- **AND THEN** all Organisation accounts SHALL immediately see the new namespace parameter

#### Scenario: Namespace parameters are read-only to consumers

- **WHEN** a domain account accesses a shared namespace parameter
- **THEN** the account SHALL be able to read the parameter value
- **AND THEN** the account SHALL NOT be able to modify or delete the parameter
