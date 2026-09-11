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

The publish API SHALL accept only `domain`, `eventName`, `data`, and an optional `idempotencyKey` in the request body. The admission step SHALL derive `source` from the domain's registered namespace identity, use `eventName` as `detail-type`, resolve the stage-selected environment's target bus and latest schema version, validate the payload, and stamp the resolved version onto the admitted message. Admission SHALL also resolve the active platform metadata revision and freeze its generated extension values on the admitted message. The publish consumer SHALL construct `detail` from that immutable information, deterministically deriving the reserved `publishedAt` field from SQS `SentTimestamp`. Callers SHALL NOT supply or override `source`, `detail-type`, target bus, metadata revision, or `detail.metadata` fields. Caller-supplied `idempotencyKey` remains the explicit supported input exception.

#### Scenario: Canonical envelope is derived

- **WHEN** a publisher sends event `OrderNotification` with business data to domain `orders` on the `prod` stage
- **THEN** the consumer SHALL publish with `source` equal to the domain's namespace identity
- **AND THEN** `detail-type` SHALL equal `OrderNotification`
- **AND THEN** the target SHALL be the platform-resolved `orders` prod bus

#### Scenario: Envelope metadata contains immutable publish fields

- **WHEN** the latest stage-selected-environment version of the schema is 4
- **THEN** the envelope `detail.metadata.schemaVersion` SHALL be numeric value `4`, equal to the version the payload was validated against at admission
- **AND THEN** `detail.metadata.idempotencyKey` SHALL be the caller-supplied key or the API Gateway request ID injected at admission
- **AND THEN** `detail.metadata.publishedAt` SHALL be the ISO 8601 conversion of the SQS `SentTimestamp`
- **AND THEN** `detail.data` SHALL contain the caller's business data
- **AND THEN** new-format events SHALL carry numeric `detail.metadata.metadataRevision` identifying their immutable metadata definition

### Requirement: Idempotency key survives the event lifecycle

The envelope `detail.metadata.idempotencyKey` SHALL be set at admission time: the caller MAY supply it in the request body, or the admission step SHALL inject the API Gateway request ID (`$context.requestId`) if omitted. The key SHALL be immutable from that point and SHALL survive SQS retries and EventBridge replays. EventBridge's native `id` field cannot be set by the producer; this field provides idempotency within the envelope.

#### Scenario: Caller-supplied idempotency key is preserved

- **WHEN** a publisher supplies `idempotencyKey` in the publish request body
- **THEN** the envelope SHALL use the caller's key
- **AND THEN** the key SHALL remain constant through retries and replays

#### Scenario: Generated idempotency key when caller omits it

- **WHEN** a publisher omits `idempotencyKey` from the publish request body
- **THEN** the admission step SHALL inject the API Gateway request ID as the key
- **AND THEN** the key SHALL remain constant through retries and replays

### Requirement: Published timestamp from SQS SentTimestamp

The envelope `detail.metadata.publishedAt` SHALL be the ISO 8601 conversion of the SQS message `SentTimestamp` attribute (epoch milliseconds). It represents the moment the event entered the durable queue, not when the consumer Lambda processed it. It SHALL be immutable from queue time and SHALL survive retries and replays.

#### Scenario: Timestamp reflects queue acceptance time

- **WHEN** a publish request is accepted by SQS at `2026-07-30T10:30:00.123Z`
- **THEN** the envelope `detail.metadata.publishedAt` SHALL equal `2026-07-30T10:30:00.123Z`
- **AND THEN** the value SHALL NOT change if the consumer processes the message later or retries it

### Requirement: Mandatory platform metadata and generated extensions

Every new-format event SHALL contain the built-in `schemaVersion`, `idempotencyKey`, `publishedAt`, and `metadataRevision` fields. Platform configuration SHALL NOT remove, redefine, shadow, or change the generation semantics of those fields. Additional metadata SHALL be generated exclusively by the platform from the configured revision and trusted runtime context; producers SHALL NOT provide extension values. All configured extension fields and object children SHALL be present and conform to their revision's generated schema.

#### Scenario: Built-ins cannot be overridden

- **WHEN** a configuration attempts to replace `publishedAt` with a user-selected generator or reuse another reserved field name
- **THEN** configuration validation SHALL reject the definition
- **AND THEN** existing definitions and active revisions SHALL remain unchanged

#### Scenario: Consistent definitions across domains

- **WHEN** sales and billing publish in an environment with metadata revision M2 active
- **THEN** both SHALL use M2's field definitions and generators
- **AND THEN** generated values MAY differ as specified by their sources, such as a distinct UUID for each admitted event

### Requirement: Immutable metadata through retries and replay

Generated extension values and metadata revision SHALL be fixed per admitted event. SQS delivery retries SHALL reuse them rather than consult current metadata configuration or generate replacements. EventBridge replays SHALL retain the recorded event and its original contract. A separately submitted publish request is a new admission, even when its business data or idempotency key repeats; Konductor SHALL NOT imply exactly-once deduplication or cross-request generator stability.

#### Scenario: Retry crosses an activation boundary

- **WHEN** an event admitted using M1 is retried after M2 becomes active
- **THEN** the event SHALL retain M1, the same UUIDs and extension timestamps, and its original `publishedAt`
- **AND THEN** M2-only fields SHALL NOT be injected

#### Scenario: Legacy event remains legacy

- **WHEN** a historical event or pre-upgrade queued message has no metadata revision
- **THEN** it SHALL be interpreted using the legacy built-in metadata contract
- **AND THEN** current metadata extensions SHALL NOT be added during delivery or replay

### Requirement: Independent business and metadata contract identity

The exact event contract SHALL be identified by organisation, environment, canonical schema name, business schema version, and metadata revision. Numeric `schemaVersion` SHALL retain its existing subscription-routing meaning. Metadata activation SHALL NOT change subscription version pins or rewrite historical business schema documents. Consumer contracts SHALL distinguish validating an exact historical revision from tolerating additional metadata fields on future events.

#### Scenario: Metadata changes without business version changes

- **WHEN** an environment activates M2 while `OrderPlaced` remains at business version V3
- **THEN** new admissions SHALL use V3 and M2
- **AND THEN** previously accepted V3/M1 events SHALL still be validated against V3/M1
- **AND THEN** existing rules filtering numeric business version 3 SHALL continue matching V3/M2 events

