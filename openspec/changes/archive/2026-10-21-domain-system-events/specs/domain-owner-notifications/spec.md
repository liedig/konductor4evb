# Spec Delta

## ADDED Requirements

### Requirement: Promotion upgrade emails for consumer owners

When optional SES notifications are enabled, promotion-triggered subscription upgrade starts and outcomes SHALL send heads-up and outcome emails to the consumer domain's primary and secondary owners. Emails SHALL identify the subscription, environment, requested version, and phase; outcomes SHALL distinguish completed upgrades, partial replay, and failed entries. Authoring-triggered upgrades SHALL not send these emails.

#### Scenario: Consumer owner receives a promotion heads-up
- **WHEN** a committed `PROMOTION` upgrade start is published
- **THEN** the configured notification path SHALL resolve the consumer domain's current owner recipients and send a heads-up
- **AND** it SHALL describe the expected re-pin time as an estimate, not a notice period

#### Scenario: Partial upgrade email reports the actual result
- **WHEN** a promotion-triggered upgrade outcome is `PARTIAL`
- **THEN** its email SHALL state the requested and resulting versions and whether the entry failed
- **AND** it SHALL NOT claim that a failed entry is delivering normally

#### Scenario: Authoring append remains machine-readable only
- **WHEN** an `AUTHORING` upgrade start or outcome is published
- **THEN** the system event SHALL remain deliverable
- **AND** this notification path SHALL send no consumer-owner promotion email

### Requirement: Promotion failure emails for producer owners

When optional SES notifications are enabled, a committed `SchemaPromotionFailed` event SHALL trigger an email to the producer domain's primary and secondary owners. It SHALL identify the schema, source and target environments, requested version, and safe failure reason. Divergence before re-pinning SHALL still notify the producer even though no consumer upgrade started.

#### Scenario: Prefix divergence fails before any subscriber changes
- **WHEN** a promotion emits `SchemaPromotionFailed` with reason `DIVERGED`
- **THEN** the producer owners SHALL receive a failure email
- **AND** the email SHALL not claim that consumers were upgraded

#### Scenario: Replay fails after writing a prefix
- **WHEN** a promotion fails after some target versions were written
- **THEN** its email SHALL identify partial progress using recorded facts
- **AND** unavailable version details SHALL be omitted rather than guessed

### Requirement: Promotion email isolation and failure classification

New promotion email paths SHALL use the existing optional SES configuration, owner recipient rules, HTML escaping, and permanent/transient error classification. Email delivery SHALL not gate schema promotion, re-pinning, or SNS delivery. Event redelivery SHALL not mutate business state; emails SHALL not promise exactly-once or chronological delivery.

#### Scenario: Notifications are disabled
- **WHEN** SES configuration is incomplete or absent
- **THEN** promotion email resources SHALL be omitted
- **AND** public system events and internal promotion outcome events SHALL remain available

#### Scenario: SES rejects a recipient permanently
- **WHEN** SES returns a permanent rejection
- **THEN** the notification SHALL be logged and acknowledged under the existing classification
- **AND** it SHALL not reverse a pin or suppress the SNS system event

#### Scenario: SES fails transiently
- **WHEN** SES returns a transient error
- **THEN** the notification invocation SHALL fail into its retry and failure-storage path
- **AND** the subscription's committed state SHALL remain unchanged
