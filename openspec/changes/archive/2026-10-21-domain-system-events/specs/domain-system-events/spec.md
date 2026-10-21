# Spec Delta

## Purpose

Provide stable, versioned platform-to-domain notifications that application teams can consume without depending on internal lifecycle events or business-event routing.

## ADDED Requirements

### Requirement: Public system-event boundary

The platform SHALL publish public system events with source `konductor.system`, a named event type, and `detail.metadata` and `detail.data` objects. Only platform-controlled publishers SHALL publish these events. Business events and internal control-plane events SHALL remain separate.

#### Scenario: Subscription change becomes a public event
- **WHEN** a committed subscription upgrade phase is projected
- **THEN** the public event SHALL use `SubscriptionUpgradeStarted` or `SubscriptionUpgraded` as its `detail-type`
- **AND** internal payload fields not defined by that public contract SHALL NOT be copied through

#### Scenario: Domain attempts to forge a system event
- **WHEN** a domain account attempts to publish directly to the system bus or a system-events topic
- **THEN** authorization SHALL deny that operation

### Requirement: Stable identity and resource ordering

Every event SHALL carry `schemaVersion`, `idempotencyKey`, `publishedAt`, `correlationId`, and `resourceRevision` in metadata. Retries of one committed phase SHALL preserve these values. Resource revisions SHALL increase for successive published upgrade states of the same subscription and environment; they need not be contiguous. Transport event identifiers SHALL NOT be the deduplication contract.

#### Scenario: Publication is retried after an ambiguous acknowledgement
- **WHEN** the platform republishes a phase that might already have been delivered
- **THEN** its idempotency key, resource revision, timestamp, and semantic payload SHALL match the earlier attempt
- **AND** consumers SHALL be able to recognize it even if the EventBridge event ID differs

#### Scenario: Events arrive out of order
- **WHEN** a consumer receives a lower resource revision after a higher revision for the same subscription and environment
- **THEN** the lower revision SHALL identify an older state that the consumer can discard
- **AND** revisions for different resources or environments SHALL NOT imply a global order

### Requirement: Explicit recipient and resource scope

Event data SHALL identify `audience`, recipient `domain`, `environment`, and `resource` with `type` and `id`. The MVP SHALL emit audience `CONSUMER`, resource type `SUBSCRIPTION`, and the existing subscription ID. Upgrade events SHALL name a concrete environment; the shared envelope SHALL allow null only for future environment-independent events.

#### Scenario: Two consumers share a producer schema
- **WHEN** one promotion upgrades subscriptions belonging to two consumer domains
- **THEN** each subscription SHALL produce its own recipient-scoped event
- **AND** each domain's topic SHALL receive only events addressed to that domain, not the other consumer's details

### Requirement: Upgrade start event

The platform SHALL publish `SubscriptionUpgradeStarted` for each materialized subscription entry whose re-pin starts after promotion validation, or after an authoring append. Data SHALL include `trigger`, `fromVersion`, `intermediateVersions`, `requestedVersion`, `resultingVersion: null`, `enabled`, and `expectedRepinAt`. The estimate SHALL NOT be a deadline, advance-notice period, or delivery-order guarantee.

#### Scenario: Promotion starts moving a paused subscription
- **WHEN** a validated promotion begins re-pinning a disabled subscription from v1 toward v3
- **THEN** the start event SHALL identify `PROMOTION`, v1, intermediate version v2, requested v3, and `enabled: false`
- **AND** publishing the notification SHALL NOT enable the subscription or delay the promotion for acknowledgement

#### Scenario: Authoring append starts re-pinning
- **WHEN** an append in the first environment starts moving a materialized subscription to the appended version
- **THEN** its start event SHALL identify `AUTHORING` and that environment
- **AND** it SHALL NOT claim that a promotion record exists

#### Scenario: Promotion is rejected before re-pinning
- **WHEN** validation rejects a divergent or otherwise invalid promotion before any subscription re-pin starts
- **THEN** no subscription-upgrade start event SHALL be published for that attempt

### Requirement: Truthful upgrade outcomes

After an entry's re-pin outcome is committed, the platform SHALL publish `SubscriptionUpgraded` with `outcome: COMPLETED | PARTIAL`, actual `resultingVersion`, and the start event's version intent. `COMPLETED` SHALL mean successful narrowing to the requested version. Partial replay or an entry failure SHALL produce `PARTIAL` with a safe reason and actual recorded state, never an invented successful pin.

#### Scenario: Replay stops before the requested version
- **WHEN** promotion from v1 toward v3 writes only v2 and the entry successfully narrows to v2
- **THEN** the outcome SHALL be `PARTIAL`, with requested v3 and resulting v2
- **AND** the reason SHALL distinguish partial replay from a failed subscription re-pin

#### Scenario: One subscription cannot narrow its rule
- **WHEN** one entry fails to re-pin while other entries succeed
- **THEN** its outcome SHALL be `PARTIAL`, identify the entry failure, and retain the actual recorded pin
- **AND** no successful delivery-state claim SHALL be made for that failed entry
- **AND** other entries SHALL receive their own outcomes independently

### Requirement: Exact contract references without schema payloads

Upgrade events SHALL carry exact contract selectors using `schemaVersion` and `metadataRevision`, including legacy metadata revision 0 where applicable. They SHALL identify producer, schema, consumer, and environment for the existing contract read API. They SHALL contain no schema documents or structural diffs and SHALL grant no additional schema-read access.

#### Scenario: Consumer fetches the resulting contract
- **WHEN** a consumer receives a completed upgrade and still has the required binding and entitlement
- **THEN** the resulting selector SHALL retrieve the exact business-version/metadata-revision pair through the existing contract API
- **AND** changing the active metadata revision later SHALL NOT change that historical selector

#### Scenario: Requested version is not present yet
- **WHEN** the start event describes a version not yet replayed into the target environment
- **THEN** that selector SHALL be identified as requested rather than available
- **AND** the event SHALL NOT imply that the target contract can already be fetched

#### Scenario: Event receipt does not bypass access checks
- **WHEN** a domain's entitlement has been revoked before its automation fetches the referenced contract
- **THEN** the existing contract API authorization SHALL still apply
- **AND** possessing the event SHALL NOT authorize the read

### Requirement: At-least-once isolated topic delivery

The platform SHALL deliver the full public EventBridge event as the SNS message body to the topic recorded for its recipient domain. Delivery SHALL be at-least-once and unordered. Temporary publication failures SHALL be retried; exhausted or invalid deliveries SHALL be observable through failure storage and alarms rather than silently acknowledged.

#### Scenario: Recipient topic is temporarily unavailable
- **WHEN** routing cannot publish to a domain's topic
- **THEN** that delivery SHALL be retried and, if exhausted, retained for diagnosis
- **AND** it SHALL NOT be rerouted to another domain or reverse a committed subscription upgrade

#### Scenario: Topic registration is missing or inconsistent
- **WHEN** the recorded topic is missing or does not identify the expected platform-owned domain topic
- **THEN** routing SHALL fail visibly without constructing an arbitrary destination from event input

### Requirement: Stable SNS filtering attributes

SNS publications SHALL include attributes `eventType`, `schemaVersion`, `audience`, `domain`, `environment`, `resourceType`, and `trigger`; terminal events SHALL additionally include `outcome`. Attribute values SHALL agree with the message body. The environment attribute SHALL use `ALL` only when the body environment is null.

#### Scenario: Consumer selects production promotion outcomes
- **WHEN** a subscription filters for `environment=prod`, `trigger=PROMOTION`, and `eventType=SubscriptionUpgraded`
- **THEN** matching messages SHALL carry those values both in their attributes and in the event body
- **AND** SNS filter configuration SHALL not change which domain the platform routes to

### Requirement: Restricted self-service subscriptions

Topic policies SHALL permit `sns:Subscribe` only to principals in the recipient domain's registered accounts and only for SQS or Lambda endpoints in those accounts or for email endpoints. HTTPS, SMS, and other protocols SHALL remain denied. Accounts SHALL require their own IAM grants and endpoint permissions. Subscription access SHALL NOT confer topic publishing, policy administration, deletion, or platform-bus rule management.

#### Scenario: Environment-specific account subscribes a queue
- **WHEN** an authorized principal in a domain's environment override account subscribes an SQS queue in a registered account with the required queue policy
- **THEN** the topic policy SHALL permit that subscription
- **AND** duplicate registered account IDs SHALL not broaden the permitted principals

#### Scenario: Unregistered account or endpoint is rejected
- **WHEN** either the subscribing account or endpoint account is outside the domain's registered set
- **THEN** the topic policy SHALL deny the subscription

#### Scenario: Email endpoint is accepted and awaits confirmation
- **WHEN** an authorized principal in a registered account subscribes an email endpoint
- **THEN** the topic policy SHALL permit that subscription
- **AND** the subscription SHALL deliver nothing until the address owner confirms it through SNS

#### Scenario: Unsupported protocol is rejected
- **WHEN** a caller requests HTTPS, SMS, or another protocol outside SQS, Lambda, and email
- **THEN** the topic policy SHALL deny the subscription even if the caller belongs to a registered account

### Requirement: Latest-event marker for console freshness

After a successful topic publish, the router SHALL record a latest-event marker (`systemEventsLastEventAt`, `systemEventsLastEventType`) on the recipient domain record, conditionally keeping the newest timestamp. The marker SHALL power the console's unread indicator only; it is not an event feed, and a marker write failure SHALL NOT fail or duplicate delivery.

#### Scenario: Newer event advances the marker
- **WHEN** the router publishes an event whose `publishedAt` is newer than the recorded marker
- **THEN** the domain record SHALL reflect the event's timestamp and detail type

#### Scenario: Duplicate or out-of-order delivery does not regress the marker
- **WHEN** a redelivered or older event reaches the router after a newer marker was recorded
- **THEN** the marker SHALL remain unchanged

#### Scenario: Marker write failure does not affect delivery
- **WHEN** the marker update fails after a successful publish
- **THEN** the failure SHALL be logged and the handler SHALL report success
- **AND** the event SHALL NOT be republished to retry the marker

### Requirement: Public schema registration and compatibility

The platform SHALL register each public event type and its contract version in a platform-managed registry, make the definitions available to integrators, and validate emitted payloads against those definitions. Changes SHALL pass the existing additive-only compatibility gate. Business-schema authoring APIs SHALL NOT modify these contracts.

#### Scenario: Compatible contract update
- **WHEN** a release adds an optional field accepted by the compatibility gate
- **THEN** the platform SHALL register the new contract version before emitting it
- **AND** identical deployment retries SHALL NOT create extra versions

#### Scenario: Breaking or unprovable contract update
- **WHEN** a release removes accepted data, tightens a constraint, or cannot prove compatibility
- **THEN** contract publication SHALL fail before the new version is emitted

### Requirement: Audit archive without MVP replay

The platform SHALL retain public system events in a central archive for 90 days. This MVP SHALL expose no replay API, replay UI, supported archive replay procedure, or automatic historical delivery to new subscribers. Consumers SHALL use current-state APIs for initialization; retries and failed-message recovery SHALL preserve event identity.

#### Scenario: Team subscribes after an earlier promotion
- **WHEN** a team creates an SNS subscription after a promotion completed
- **THEN** it SHALL receive subsequent matching notifications, not an automatic replay of earlier upgrades
- **AND** it SHALL initialize its state through the existing subscription and contract APIs

#### Scenario: Notification capability is independent of SES
- **WHEN** the platform is deployed without email notification parameters
- **THEN** system-event projection, archive, and SNS delivery SHALL remain available
