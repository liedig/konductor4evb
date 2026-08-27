# Capability: Event Envelope

## Purpose

The canonical envelope every domain event carries: platform-derived `source` identity, the `{source}@{detail-type}` schema naming convention, numeric schema-version stamping under `detail.metadata`, event-name constraints inherited from derived rule names, and the immutable publish-time metadata (idempotency key and published timestamp) that survives retries and replays.

## Requirements

### Requirement: Canonical event identity

Every event flowing through Konductor domain buses SHALL carry a canonical identity: `source` SHALL be the producing domain's environment-independent namespace identity `{org}-{domain}` (e.g., `prezzee-orders`), and `detail-type` SHALL be the event name. The `source` value SHALL be derived by the platform from the domain's registered namespace identity — it SHALL NOT be free-typed by producers. The identity SHALL be identical across environments; only resource names (buses, registries) carry the environment segment.

#### Scenario: Source derived from namespace identity

- **WHEN** the platform derives the `source` for events of the `orders` domain in organization `prezzee`
- **THEN** the value SHALL be `prezzee-orders`, matching the domain's SSM namespace identity
- **AND THEN** the same value SHALL be used in every environment

#### Scenario: Envelope identity is environment-portable

- **WHEN** a subscription rule pattern is generated for any environment
- **THEN** the `source` and `detail-type` filter values SHALL be identical across environments

### Requirement: Schema naming convention

Event schemas in domain registries SHALL be named `{source}@{detail-type}` (e.g., `prezzee-orders@OrderNotification`), following the native EventBridge schema-discovery convention. This name SHALL round-trip directly to the AWS Schemas API: `aws schemas describe-schema --registry-name {org}-{env}-{domain}-schema-registry --schema-name {source}@{detail-type} --schema-version N`. The schema name alone SHALL be sufficient to derive a subscription's complete rule filter.

#### Scenario: Schema name encodes the filter

- **WHEN** a consumer subscribes to schema `prezzee-orders@OrderNotification`
- **THEN** the platform SHALL split the name on `@` to derive `source: prezzee-orders` and `detail-type: OrderNotification`
- **AND THEN** no independent filter parameters SHALL be accepted from the caller

#### Scenario: Schema identity resolves in the registry

- **WHEN** the activation workflow resolves the latest version of `prezzee-orders@OrderNotification` in the prod registry
- **THEN** it SHALL call the Schemas API against registry `{org}-prod-orders-schema-registry` with schema name `prezzee-orders@OrderNotification`

### Requirement: Schema version stamping

Events SHALL carry their schema version as a **numeric** value at `detail.metadata.schemaVersion`. The top-level EventBridge `version` field is AWS-reserved and SHALL NOT be used for schema versioning. The event `detail` SHALL follow a `metadata`/`data` split, where `metadata` carries platform concerns (at minimum the schema version) and `data` carries the business payload. The future publish capability MUST stamp and validate this envelope; subscription filtering is only as trustworthy as publish-side stamping.

#### Scenario: Numeric version enables rule matching

- **WHEN** an event is published with `detail.metadata.schemaVersion: 3` (numeric)
- **THEN** a subscription rule pinning version 3 SHALL match the event
- **AND THEN** numeric stamping SHALL keep EventBridge numeric range matching (e.g., `{"numeric": ["<=", 3]}`) available for future evolution use

#### Scenario: Mis-stamped events fail closed

- **WHEN** an event omits `detail.metadata.schemaVersion` or stamps it as a string
- **THEN** version-pinned subscription rules SHALL NOT match the event
- **AND THEN** the failure mode SHALL be non-delivery, never delivery of a non-conforming event

### Requirement: Event name constraints

Event names (`detail-type` values) SHALL be validated when a publisher creates a schema. Characters SHALL be limited to `[A-Za-z0-9._-]`, and schema creation SHALL reject an event name that cannot form a `{consumerDomain}-{detailType}` rule within EventBridge's 64-character limit with any valid consumer-domain name. Because a target consumer is not known at schema-creation time, subscription creation SHALL validate the actual derived rule name.

#### Scenario: Invalid characters rejected at schema creation

- **WHEN** a publisher attempts to create a schema whose event name contains a space
- **THEN** schema creation SHALL fail validation with an error identifying the allowed characters
- **AND THEN** no schema SHALL be created

#### Scenario: Intrinsically oversized event name rejected

- **WHEN** a publisher attempts to create a schema whose event name cannot produce a valid `{consumerDomain}-{detailType}` rule name with any valid consumer domain
- **THEN** schema creation SHALL fail validation with an error identifying the length constraint

#### Scenario: Subscription retains defensive validation

- **WHEN** a consumer subscribes to an existing schema whose derived rule name violates EventBridge constraints
- **THEN** the subscribe command SHALL fail validation rather than attempting to create an invalid rule

### Requirement: Publish API owns canonical envelope construction

The publish API SHALL accept only `domain`, `eventName`, `data`, and an optional `idempotencyKey` in the request body. The publish consumer SHALL derive `source` from the domain's registered namespace identity, use `eventName` as `detail-type`, resolve the stage-selected environment's target bus and latest schema version, and construct `detail` with a platform-owned `metadata` object and the caller's `data`. Callers SHALL NOT supply or override `source`, `detail-type`, target bus, or `detail.metadata` fields.

#### Scenario: Canonical envelope is derived

- **WHEN** a publisher sends event `OrderNotification` with business data to domain `orders` on the `prod` stage
- **THEN** the consumer SHALL publish with `source` equal to the domain's namespace identity
- **AND THEN** `detail-type` SHALL equal `OrderNotification`
- **AND THEN** the target SHALL be the platform-resolved `orders` prod bus

#### Scenario: Envelope metadata contains immutable publish fields

- **WHEN** the latest stage-selected-environment version of the schema is 4
- **THEN** the envelope `detail.metadata.schemaVersion` SHALL be numeric value `4`
- **AND THEN** `detail.metadata.idempotencyKey` SHALL be the caller-supplied key or the API Gateway request ID
- **AND THEN** `detail.metadata.publishedAt` SHALL be the ISO 8601 conversion of the SQS `SentTimestamp`
- **AND THEN** `detail.data` SHALL contain the caller's business data

### Requirement: Idempotency key survives the event lifecycle

The envelope `detail.metadata.idempotencyKey` SHALL be set at API Gateway time: the caller MAY supply it in the request body, or the VTL mapping template SHALL inject `$context.requestId` if omitted. The key SHALL be immutable from that point and SHALL survive SQS retries and EventBridge replays. EventBridge's native `id` field cannot be set by the producer; this field provides idempotency within the envelope.

#### Scenario: Caller-supplied idempotency key is preserved

- **WHEN** a publisher supplies `idempotencyKey` in the publish request body
- **THEN** the envelope SHALL use the caller's key
- **AND THEN** the key SHALL remain constant through retries and replays

#### Scenario: Generated idempotency key when caller omits it

- **WHEN** a publisher omits `idempotencyKey` from the publish request body
- **THEN** the VTL mapping template SHALL inject `$context.requestId` as the key
- **AND THEN** the key SHALL remain constant through retries and replays

### Requirement: Published timestamp from SQS SentTimestamp

The envelope `detail.metadata.publishedAt` SHALL be the ISO 8601 conversion of the SQS message `SentTimestamp` attribute (epoch milliseconds). It represents the moment the event entered the durable queue, not when the consumer Lambda processed it. It SHALL be immutable from queue time and SHALL survive retries and replays.

#### Scenario: Timestamp reflects queue acceptance time

- **WHEN** a publish request is accepted by SQS at `2026-07-30T10:30:00.123Z`
- **THEN** the envelope `detail.metadata.publishedAt` SHALL equal `2026-07-30T10:30:00.123Z`
- **AND THEN** the value SHALL NOT change if the consumer processes the message later or retries it

### Requirement: Publish-time validation is envelope-only

Before calling EventBridge, the publish consumer SHALL validate the complete generated envelope, including canonical identity, numeric schema version, required `metadata`/`data` structure, serializability, and EventBridge entry constraints. It SHALL NOT validate the business `data` against the schema document.

#### Scenario: Business data is not schema-validated

- **WHEN** an authorized publisher supplies JSON business data that does not satisfy the registered schema
- **THEN** the consumer SHALL NOT reject the request on schema-conformance grounds
- **AND THEN** the consumer SHALL continue envelope validation and publication

#### Scenario: Invalid generated envelope fails before delivery

- **WHEN** the complete generated envelope violates a required envelope or EventBridge entry constraint
- **THEN** the consumer SHALL treat the message as a processing failure
- **AND THEN** the consumer SHALL NOT call EventBridge for that event
