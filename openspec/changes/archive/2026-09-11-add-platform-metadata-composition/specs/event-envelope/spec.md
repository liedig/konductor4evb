## MODIFIED Requirements

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

## ADDED Requirements

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
