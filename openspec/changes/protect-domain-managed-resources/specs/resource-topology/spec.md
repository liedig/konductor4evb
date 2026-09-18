## MODIFIED Requirements

### Requirement: Application team responsibility boundary

Application teams SHALL only interact directly with domain event buses in their own account for integration-rule management. They SHALL create integration rules on those buses to process events, including triggering Lambda functions, SQS queues, or other targets. Application teams SHALL NOT create or manage rules on the platform domain bus. Konductor SHALL retain management of the application bus infrastructure and integration DLQ configuration. In member accounts with the organization guardrails installed, application teams, including local administrators within SCP coverage, SHALL NOT delete or reconfigure those managed resources.

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
