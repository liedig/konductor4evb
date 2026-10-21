# Spec Delta

## ADDED Requirements

### Requirement: Latest system-event marker discovery

The domain list and detail APIs SHALL expose optional `systemEventsLastEventAt` and `systemEventsLastEventType` from the domain record so the console can compute its unread indicator from the list endpoint alone. The marker SHALL appear only when recorded and SHALL remain absent for domains without any delivered event. The topic ARN SHALL remain describe-only.

#### Scenario: List carries the marker but not the ARN
- **WHEN** an authorized caller lists domains and one has a recorded marker
- **THEN** the summary SHALL contain the marker timestamp and event type
- **AND** it SHALL still omit `systemEventsTopicArn`

#### Scenario: No events delivered yet
- **WHEN** a domain has a topic but no recorded marker
- **THEN** both APIs SHALL omit the marker fields
- **AND** the console SHALL treat the domain as seen

### Requirement: Domain system-events topic discovery

The OAuth and IAM domain-detail APIs SHALL expose optional top-level `systemEventsTopicArn` from the domain record. The field SHALL appear only after topic configuration is recorded and SHALL not be synthesized from a naming convention. It SHALL remain outside per-environment resource maps and preserve existing domain visibility rules.

#### Scenario: Owner discovers a configured topic
- **WHEN** an authorized domain owner describes a domain whose topic has been provisioned or backfilled
- **THEN** the response SHALL contain the recorded topic ARN once at domain scope
- **AND** that ARN SHALL be identical regardless of API stage

#### Scenario: Topic has not been configured
- **WHEN** an authorized caller describes a domain with no recorded topic ARN
- **THEN** the response SHALL omit `systemEventsTopicArn`
- **AND** the existing domain details SHALL remain readable

#### Scenario: Both authentication surfaces expose the field
- **WHEN** authorized OAuth and IAM callers describe the same domain
- **THEN** both response contracts SHALL expose the same recorded topic ARN
- **AND** the new field SHALL not introduce a new authorization path

#### Scenario: Topic discovery does not leak through directory lookup
- **WHEN** a caller uses the names-only domain directory or cannot access a domain's detail
- **THEN** the system SHALL not reveal the topic ARN through that response
- **AND** unauthorized detail requests SHALL preserve the existing not-found or access-denied behavior

### Requirement: Owner email change reconciles the topic subscription

A committed `ownerEmail` change SHALL trigger reconciliation of the domain's system-events topic default email subscription: the previous address is unsubscribed and the new address subscribed through the platform's topic-management path. The reconciliation SHALL NOT be accepted as a mutable domain field or performed by domain callers directly, and its failure SHALL NOT reverse the committed update.

#### Scenario: Owner change re-points the default subscription
- **WHEN** an authorized caller commits an owner-email change for a domain with a recorded topic
- **THEN** the topic's default email subscription SHALL be moved from the old address to the new one
- **AND** the success event SHALL carry both `ownerEmail` and `previousOwnerEmail` so the reconciliation consumer never guesses the replaced address
- **AND** the new address SHALL receive a fresh SNS confirmation request

#### Scenario: Reconciliation failure does not roll back the update
- **WHEN** subscription reconciliation fails after the owner change committed
- **THEN** the domain record SHALL retain the new owner email
- **AND** the failure SHALL remain observable for operator retry

#### Scenario: Domain without a topic skips reconciliation
- **WHEN** the owner email changes for a domain with no recorded topic ARN
- **THEN** reconciliation SHALL be skipped without failing the update

### Requirement: Topic configuration remains platform-owned

Domain registration and update APIs SHALL not accept system-event topic destinations, topic policies, or endpoint subscriptions as mutable domain fields. Discovering an ARN SHALL not itself authorize SNS access; domain accounts SHALL subscribe through AWS using the topic policy and their own permissions.

#### Scenario: Caller supplies a replacement topic
- **WHEN** a domain update attempts to replace `systemEventsTopicArn`
- **THEN** it SHALL not change the stored topic or its policy
- **AND** public update schemas SHALL not declare the field writable
