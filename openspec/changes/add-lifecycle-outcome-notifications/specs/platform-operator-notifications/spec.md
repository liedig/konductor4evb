# Delta: platform-operator-notifications

## ADDED Requirements

### Requirement: Platform notifications topic

The platform SHALL provide an SNS topic named `nexus-platform-notifications` as the single subscription point for platform-operator notifications. The topic SHALL carry a display name identifying the platform (`Nexus Platform`). An email subscription for the `PlatformOperatorEmail` parameter SHALL be created with the topic. The topic, its policy, and the subscription SHALL deploy only when notifications are enabled (the existing `NotificationsEnabled` condition). Future notification channel adapters (e.g. Slack, SMTP relays) SHALL integrate as additional subscribers to this topic and are out of scope for this capability.

#### Scenario: Topic and operator subscription deployed with notifications enabled

- **WHEN** the core stack is deployed with notification parameters configured
- **THEN** the `nexus-platform-notifications` SNS topic SHALL exist
- **AND THEN** an email-protocol subscription for `PlatformOperatorEmail` SHALL exist on the topic

#### Scenario: Notifications disabled

- **WHEN** the core stack is deployed without notification parameters
- **THEN** the topic, topic policy, subscription, and forwarding rule SHALL NOT be deployed

#### Scenario: Subscription requires one-time confirmation

- **WHEN** the stack is first deployed and the subscription is created
- **THEN** the operator SHALL receive an SNS confirmation email and messages SHALL NOT be delivered to that endpoint until confirmed
- **AND THEN** deployment documentation SHALL call out the confirmation step

### Requirement: Lifecycle event forwarding to operators

All lifecycle events on the nexus bus SHALL be forwarded to the platform notifications topic by a single EventBridge rule matching `source` values with the prefix `nexus.` (covering `nexus.domain-management`, `nexus.event-streams`, and any future platform source). The rule SHALL use an input transformer producing a compact JSON message containing the detail-type, source, event time, and the full event detail, using only envelope fields guaranteed present on every event. The rule target SHALL carry a `DeadLetterConfig` referencing the EventBridge rule failure DLQ. A topic policy SHALL permit `events.amazonaws.com` to publish, scoped to the forwarding rule's ARN.

#### Scenario: Domain-management lifecycle event forwarded

- **WHEN** any event with source `nexus.domain-management` (e.g. `DomainProvisioningSucceeded`, `DomainApprovalNotificationFailed`) is published to the nexus bus
- **THEN** the rule SHALL match and publish a message to `nexus-platform-notifications`
- **AND THEN** the message SHALL contain the detail-type, source, event time, and full event detail

#### Scenario: Event-streams lifecycle event forwarded

- **WHEN** any event with source `nexus.event-streams` is published to the nexus bus
- **THEN** the rule SHALL match and publish a message to the topic without any per-detail-type configuration

#### Scenario: Operator delivery independent of owner email path

- **WHEN** a `DomainApprovalNotificationFailed` event fires because SES delivery failed
- **THEN** the operator message SHALL still be published via SNS, which does not depend on SES

#### Scenario: Delivery failure lands in DLQ

- **WHEN** EventBridge fails to publish a matched event to the topic after retries
- **THEN** the event SHALL be delivered to the EventBridge rule failure DLQ
