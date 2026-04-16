# resource-topology Specification

## Purpose
TBD - created by archiving change architecture-decisions. Update Purpose after archive.
## Requirements
### Requirement: Platform account domain resources

When a domain is provisioned, EventBridge Nexus SHALL create the following resources in the platform account: a domain event bus, a PutEvent policy restricting publishing to the domain's owning application account, a schema registry with access policies, a subscription rule DLQ (SQS queue), and CloudWatch observability resources (dashboard, alarms, metrics configuration).

#### Scenario: Domain event bus created in platform account

- **WHEN** the domain provisioning workflow creates the domain event bus in the platform account
- **THEN** the bus SHALL be named according to the `{org}-{env}-{domain}-domain-bus` convention
- **AND THEN** the bus SHALL exist in the platform account where EventBridge Nexus operates

#### Scenario: PutEvent policy restricts publishing to domain owner

- **WHEN** the domain event bus is created in the platform account
- **THEN** the Nexus SHALL attach a resource policy to the bus that permits `events:PutEvents` only from the domain's owning application account
- **AND THEN** no other application account SHALL be able to publish events to that domain bus

#### Scenario: Schema registry created per domain

- **WHEN** the domain is provisioned
- **THEN** the Nexus SHALL create a schema registry in the platform account named according to the `{org}-{env}-{domain}-schema-registry` convention
- **AND THEN** the registry SHALL have access policies granting write access to the domain owner and read access to authorised consumers

#### Scenario: Subscription rule DLQ created in platform account

- **WHEN** the domain is provisioned
- **THEN** the Nexus SHALL create an SQS queue in the platform account named `{org}-{env}-{domain}-subscription-dlq` to serve as the dead-letter queue for subscription rules on the domain event bus
- **AND THEN** failed deliveries from subscription rules (domain bus to subscriber bus) SHALL be routed to this DLQ

#### Scenario: Observability resources created per domain

- **WHEN** the domain is provisioned
- **THEN** the Nexus SHALL create a CloudWatch dashboard, alarms, and metrics configuration for the domain event bus in the platform account

### Requirement: Application account domain resources

When a domain is provisioned, EventBridge Nexus SHALL create the following resources in the domain's owning application account: a domain event bus (where the app receives routed events) and an integration rule DLQ (SQS queue).

#### Scenario: Domain event bus created in application account

- **WHEN** the domain provisioning workflow runs
- **THEN** the Nexus SHALL create a domain event bus in the owning application account, named according to the `{org}-{env}-{domain}-domain-bus` convention
- **AND THEN** this bus SHALL be the target for subscription rules that route events from the platform domain bus to the application account

#### Scenario: Integration rule DLQ created in application account

- **WHEN** the domain provisioning workflow runs
- **THEN** the Nexus SHALL create an SQS queue in the application account named `{org}-{env}-{domain}-integration-dlq` to serve as the dead-letter queue for integration rules
- **AND THEN** failed deliveries from integration rules (app bus to consuming application targets like Lambda or SQS) SHALL be routed to this DLQ

### Requirement: One domain bus per subscribing application account

Each subscribing application account SHALL receive events on a dedicated domain event bus per domain. There SHALL be a 1:1 mapping between domain and event bus per application account. Multiple domains operating within a single application account SHALL each have their own separate domain event bus in that account.

#### Scenario: Multiple domains in one application account

- **WHEN** an application account owns or subscribes to multiple domains (e.g., billing and sales)
- **THEN** each domain SHALL have its own dedicated event bus in that application account
- **AND THEN** events for each domain SHALL be routed to the corresponding domain-specific bus

#### Scenario: Subscription routing targets application account bus

- **WHEN** a subscription is created for a consumer in a different application account
- **THEN** the subscription rule on the platform domain bus SHALL target the domain event bus in the subscriber's application account
- **AND THEN** the routing SHALL use a bus-to-bus assumed role to deliver events cross-account

### Requirement: Application team responsibility boundary

Application teams SHALL only interact with the domain event bus in their own account. They SHALL create integration rules on their own bus to process events (e.g., triggering Lambda functions, SQS queues, or other targets). Application teams SHALL NOT create or manage rules on the platform domain bus.

#### Scenario: Application team creates integration rules

- **WHEN** events are delivered to the domain event bus in the application account
- **THEN** the application team SHALL create EventBridge rules on their own domain bus to route events to their consuming applications
- **AND THEN** the Nexus SHALL NOT manage or interfere with integration rules created by the application team

#### Scenario: Application team cannot access platform domain bus

- **WHEN** an application team attempts to create rules or modify the domain event bus in the platform account
- **THEN** IAM policies SHALL deny the operation
- **AND THEN** only the Nexus SHALL have permission to manage rules and policies on the platform domain bus

### Requirement: Event flow path

Events SHALL flow through a three-stage path: (1) the publishing application calls PutEvents on the domain event bus in the platform account, (2) subscription rules on the platform domain bus match and route events to the subscriber's domain event bus in their application account via bus-to-bus assumed roles, (3) the subscriber's integration rules on their own bus deliver events to their consuming applications.

#### Scenario: End-to-end event delivery

- **WHEN** a publishing application sends an event to the domain event bus in the platform account
- **THEN** the event SHALL be evaluated against subscription rules on that bus
- **AND THEN** matching subscription rules SHALL deliver the event to the target domain event bus in the subscriber's application account using a bus-to-bus assumed role
- **AND THEN** the subscriber's integration rules SHALL process the event on their own bus

#### Scenario: Subscription rule delivery failure

- **WHEN** a subscription rule on the platform domain bus fails to deliver an event to the subscriber's application account bus
- **THEN** the event SHALL be sent to the subscription rule DLQ in the platform account
- **AND THEN** the DLQ message SHALL include error details (error code, error message, rule ARN, target ARN)

#### Scenario: Integration rule delivery failure

- **WHEN** an integration rule on the subscriber's domain bus fails to deliver an event to the consuming application
- **THEN** the event SHALL be sent to the integration rule DLQ in the application account
- **AND THEN** the DLQ message SHALL include error details for troubleshooting

### Requirement: Domain namespace convention

Domain resources SHALL be named according to the `{org}-{env}-{domain}` convention (e.g., `acme-dev-billing`). Platform teams SHALL be able to configure this naming pattern. The convention SHALL support multiple environments (dev, staging, prod) to allow resource isolation per environment. The domain namespace (environment-independent identity) SHALL be `{org}-{domain}`.

#### Scenario: Default namespace applied to resources

- **WHEN** a domain is provisioned with org `acme`, domain name `billing`, and environment `dev`
- **THEN** the domain event bus in the platform account SHALL be named `acme-dev-billing-domain-bus` (or according to the configured convention)
- **AND THEN** the schema registry, DLQs, and other resources SHALL follow the same convention with appropriate suffixes

#### Scenario: Platform team configures custom convention

- **WHEN** a platform team specifies a custom namespace convention
- **THEN** all subsequently provisioned domain resources SHALL use the custom convention for naming
- **AND THEN** existing resources SHALL NOT be renamed (convention changes are forward-only)

#### Scenario: Environment isolation

- **WHEN** the same domain (e.g., `billing`) is provisioned across dev, staging, and prod environments
- **THEN** each environment SHALL have its own independent set of resources (buses, registries, DLQs)
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

All domain namespace SSM parameters SHALL be shared with the AWS Organisation via a single AWS RAM resource share. Application accounts SHALL have read-only access to namespace parameters without requiring cross-account IAM configuration.

#### Scenario: RAM share contains all namespace parameters

- **WHEN** multiple domains are provisioned (e.g., billing, orders, payments)
- **THEN** a single RAM resource share named `{org}-domain-namespaces` SHALL contain the SSM parameters for all domain namespaces
- **AND THEN** all accounts in the AWS Organisation SHALL be principals on the share

#### Scenario: New namespace automatically shared

- **WHEN** a new domain is provisioned and its namespace parameter is created
- **THEN** the parameter SHALL be automatically associated with the existing RAM resource share
- **AND THEN** all Organisation accounts SHALL immediately see the new namespace parameter

#### Scenario: Namespace parameters are read-only to consumers

- **WHEN** an application account accesses a shared namespace parameter
- **THEN** the account SHALL be able to read the parameter value
- **AND THEN** the account SHALL NOT be able to modify or delete the parameter

