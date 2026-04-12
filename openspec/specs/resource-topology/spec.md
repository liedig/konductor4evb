# resource-topology Specification

## Purpose
TBD - created by archiving change architecture-decisions. Update Purpose after archive.
## Requirements
### Requirement: Platform account domain resources

When a domain is provisioned, the control plane SHALL create the following resources in the platform account: a domain event bus, a PutEvent policy restricting publishing to the domain's owning application account, a schema registry with access policies, a subscription rule DLQ (SQS queue), and CloudWatch observability resources (dashboard, alarms, metrics configuration).

#### Scenario: Domain event bus created in platform account

- **WHEN** the domain provisioning workflow creates the domain event bus in the platform account
- **THEN** the bus SHALL be named according to the configurable namespace convention (e.g., `{env}-{org}-{domain}-domain-bus`)
- **AND THEN** the bus SHALL exist in the platform account where the control plane operates

#### Scenario: PutEvent policy restricts publishing to domain owner

- **WHEN** the domain event bus is created in the platform account
- **THEN** the control plane SHALL attach a resource policy to the bus that permits `events:PutEvents` only from the domain's owning application account
- **AND THEN** no other application account SHALL be able to publish events to that domain bus

#### Scenario: Schema registry created per domain

- **WHEN** the domain is provisioned
- **THEN** the control plane SHALL create a schema registry in the platform account named according to the namespace convention
- **AND THEN** the registry SHALL have access policies granting write access to the domain owner and read access to authorized consumers

#### Scenario: Subscription rule DLQ created in platform account

- **WHEN** the domain is provisioned
- **THEN** the control plane SHALL create an SQS queue in the platform account to serve as the dead-letter queue for subscription rules on the domain event bus
- **AND THEN** failed deliveries from subscription rules (domain bus to subscriber bus) SHALL be routed to this DLQ

#### Scenario: Observability resources created per domain

- **WHEN** the domain is provisioned
- **THEN** the control plane SHALL create a CloudWatch dashboard, alarms, and metrics configuration for the domain event bus in the platform account

### Requirement: Application account domain resources

When a domain is provisioned, the control plane SHALL create the following resources in the domain's owning application account: a domain event bus (where the app receives routed events) and an integration rule DLQ (SQS queue).

#### Scenario: Domain event bus created in application account

- **WHEN** the domain provisioning workflow runs
- **THEN** the control plane SHALL create a domain event bus in the owning application account, named according to the namespace convention
- **AND THEN** this bus SHALL be the target for subscription rules that route events from the platform domain bus to the application account

#### Scenario: Integration rule DLQ created in application account

- **WHEN** the domain provisioning workflow runs
- **THEN** the control plane SHALL create an SQS queue in the application account to serve as the dead-letter queue for integration rules
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
- **AND THEN** the control plane SHALL NOT manage or interfere with integration rules created by the application team

#### Scenario: Application team cannot access platform domain bus

- **WHEN** an application team attempts to create rules or modify the domain event bus in the platform account
- **THEN** IAM policies SHALL deny the operation
- **AND THEN** only the control plane SHALL have permission to manage rules and policies on the platform domain bus

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

Domain resources SHALL be named according to a configurable namespace convention. The default convention SHALL be `{env}-{org}-{domain}` (e.g., `prod-acme-billing`). Platform teams SHALL be able to configure this naming pattern. The convention SHALL support multiple environments (dev, staging, prod) to allow resource isolation per environment.

#### Scenario: Default namespace applied to resources

- **WHEN** a domain is provisioned with org "acme", domain name "billing", and environment "prod"
- **THEN** the domain event bus in the platform account SHALL be named `prod-acme-billing-domain-bus` (or according to the configured convention)
- **AND THEN** the schema registry, DLQs, and other resources SHALL follow the same convention with appropriate suffixes

#### Scenario: Platform team configures custom convention

- **WHEN** a platform team specifies a custom namespace convention (e.g., `{org}/{env}/{domain}` or `{domain}-{env}`)
- **THEN** all subsequently provisioned domain resources SHALL use the custom convention for naming
- **AND THEN** existing resources SHALL NOT be renamed (convention changes are forward-only)

#### Scenario: Environment isolation

- **WHEN** the same domain (e.g., "billing") is provisioned across dev, staging, and prod environments
- **THEN** each environment SHALL have its own independent set of resources (buses, registries, DLQs)
- **AND THEN** there SHALL be no cross-environment event routing unless explicitly configured

