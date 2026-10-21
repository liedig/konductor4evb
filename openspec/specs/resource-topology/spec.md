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

Application teams SHALL manage business-event integration rules only on domain event buses in their own account. They SHALL create integration rules on those buses to process events, including triggering Lambda functions, SQS queues, or other targets. Application teams SHALL NOT create or manage rules on the platform domain bus. Konductor SHALL retain management of the application bus infrastructure and integration DLQ configuration. In member accounts with the organization guardrails installed, application teams, including local administrators within SCP coverage, SHALL NOT delete or reconfigure those managed resources.

As a separate system-notification path, principals in a domain's registered accounts SHALL be permitted, with the necessary IAM and endpoint permissions, to subscribe allowed SQS and Lambda endpoints to that domain's platform-owned system-events SNS topic. This permission SHALL NOT confer topic administration, publication rights, or access to manage platform event-bus rules.

#### Scenario: Application team creates integration rules

- **WHEN** events are delivered to the domain event bus in the domain account
- **THEN** the application team SHALL be able, with the necessary IAM grants, to create, update, enable, disable, and delete its integration rules and manage their targets
- **AND THEN** Konductor SHALL NOT manage or interfere with integration rules created by the application team

#### Scenario: Application team cannot access platform domain bus

- **WHEN** an application team attempts to create rules or modify the domain event bus in the platform account
- **THEN** IAM policies SHALL deny the operation
- **AND THEN** only Konductor SHALL have permission to manage rules and policies on the platform domain bus

#### Scenario: Application team consumes integration DLQ messages

- **WHEN** an application principal has IAM permission to consume its integration DLQ
- **THEN** the guardrails SHALL preserve receiving messages, acknowledging them by deletion, changing their visibility, and reading queue attributes and URLs, including supported batch equivalents
- **AND** those operations SHALL NOT confer permission to delete or purge the queue or change its attributes, policy, or redrive configuration

#### Scenario: Local administration does not imply infrastructure ownership

- **WHEN** a domain-account administrator attempts to delete or reconfigure a Konductor-managed application bus or integration DLQ in a protected member account
- **THEN** the operation SHALL be denied unless performed through the explicitly authorized platform management or recovery path
- **AND** the team's permitted integration operations SHALL remain available

#### Scenario: Unrelated application resources are unaffected

- **WHEN** an application team manages resources outside the protected Konductor selectors
- **THEN** these guardrails SHALL NOT deny the operation merely because the resources share the organization's general naming prefix

#### Scenario: Team connects system-event automation without platform-bus access

- **WHEN** an authorized domain-account principal subscribes a permitted endpoint to its domain's system-events topic
- **THEN** SNS subscription authorization SHALL be evaluated independently of business-event integration-rule permissions
- **AND** platform event-bus rule management SHALL remain denied

### Requirement: Domain namespace convention

Domain resources SHALL be named according to a configurable pattern defaulting to `{org}-{env}-{domain}` (e.g., `acme-dev-billing`). Platform teams SHALL be able to configure this naming pattern. The convention SHALL support multiple environments (dev, staging, prod) to allow resource isolation per environment at the domain layer. The domain namespace (environment-independent identity) SHALL be `{org}-{domain}`.

Konductor platform resources — the control plane itself — SHALL NOT use this convention. Platform resources follow the fixed `{org}-konductor-*` pattern (see system-architecture spec) because Konductor is a singleton per organisation and has no environment concept at the platform layer.

The domain-wide system-events SNS topic SHALL be an explicit environment-independent exception: its fixed name SHALL be `{org}-{domain}-system-events`, derived from the domain namespace rather than the configurable per-environment pattern.

#### Scenario: Default namespace applied to domain resources

- **WHEN** a domain is provisioned with org `acme`, domain name `billing`, and environment `dev`
- **THEN** the domain event bus in the platform account SHALL be named `acme-dev-billing-event-bus` (or according to the configured convention)
- **AND THEN** the schema registry, DLQs, and other per-environment domain resources SHALL follow the same convention with appropriate suffixes

#### Scenario: Platform team configures custom convention

- **WHEN** a platform team specifies a custom namespace convention
- **THEN** all subsequently provisioned per-environment domain resources SHALL use the custom convention for naming
- **AND THEN** existing resources SHALL NOT be renamed (convention changes are forward-only)
- **AND THEN** custom conventions SHALL apply only to per-environment domain resources; Konductor platform resources SHALL remain named `{org}-konductor-*`
- **AND THEN** the domain-wide system-events topic SHALL retain `{org}-{domain}-system-events`

#### Scenario: Environment isolation between domain resources

- **WHEN** the same domain (e.g., `billing`) is provisioned across dev, staging, and prod environments
- **THEN** each environment SHALL have its own independent set of business-event resources (buses, registries, DLQs)
- **AND THEN** there SHALL be no cross-environment business-event routing unless explicitly configured
- **AND THEN** the domain namespace `{org}-{domain}` SHALL be the same across all environments
- **AND THEN** sharing a system-events topic SHALL NOT create cross-environment business-event routing
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

### Requirement: System-event resources

The platform SHALL own one `{org}-konductor-system-bus`, its audit archive, and a `{org}-konductor-system-schemas` registry per installation. Each provisioned domain SHALL have one platform-account SNS topic named `{org}-{domain}-system-events`, shared across all its environments. These resources SHALL be separate from business-event buses, internal lifecycle buses, and operator notification topics.

#### Scenario: Multi-environment domain has one notification topic
- **WHEN** a domain is provisioned into dev, staging, and prod
- **THEN** it SHALL have one system-events topic, not three
- **AND** its existing per-environment business resources SHALL remain distinct

#### Scenario: Topic count grows without per-domain system-bus rules
- **WHEN** a platform provisions additional domains up to its 1,000-domain cap
- **THEN** each domain SHALL receive its own topic
- **AND** recipient routing SHALL NOT require one system-bus rule per domain

#### Scenario: System notifications remain available without email
- **WHEN** optional SES or operator email notifications are disabled
- **THEN** the system bus, archive, registry, and domain topics SHALL still be provisioned
