# Capability: Resource Topology (delta)

## ADDED Requirements

### Requirement: Subscription rules on platform domain buses

Subscription rules SHALL live on the producer's platform-side domain event bus, one rule per subscription per environment, named `{consumerDomain}-{detailType}`. Rule patterns SHALL filter on the canonical envelope (`source`, `detail-type`, numeric `detail.metadata.schemaVersion` pinned per environment). Only the Nexus SHALL manage these rules. The `nexus-event-streams` DynamoDB table SHALL be the authoritative registry of subscription topology; the rules are its materialization.

#### Scenario: Rule placement and naming

- **WHEN** the `checkout` domain subscribes to `prezzee-orders@OrderNotification`
- **THEN** each environment's platform bus `{org}-{env}-orders-event-bus` SHALL carry exactly one rule named `checkout-OrderNotification` for this subscription

#### Scenario: Topology queried from the table, not from AWS resources

- **WHEN** any process needs the set of active event streams (per producer, per consumer, or per schema version)
- **THEN** it SHALL query the `nexus-event-streams` table
- **AND THEN** it SHALL NOT enumerate EventBridge rules across buses to derive topology

## MODIFIED Requirements

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
