# Spec Delta

## ADDED Requirements

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

## MODIFIED Requirements

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
