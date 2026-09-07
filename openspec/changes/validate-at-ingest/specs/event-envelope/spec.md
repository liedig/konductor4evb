## MODIFIED Requirements

### Requirement: Publish API owns canonical envelope construction

The publish API SHALL accept only `domain`, `eventName`, `data`, and an optional `idempotencyKey` in the request body. The admission step SHALL derive `source` from the domain's registered namespace identity, use `eventName` as `detail-type`, resolve the stage-selected environment's target bus and latest schema version, validate the payload, and stamp the resolved version onto the admitted message; the publish consumer SHALL construct `detail` with a platform-owned `metadata` object and the caller's `data` from the admitted attributes. Callers SHALL NOT supply or override `source`, `detail-type`, target bus, or `detail.metadata` fields.

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

## REMOVED Requirements

### Requirement: Publish-time validation is envelope-only

**Reason**: Superseded by payload conformance validation at admission (see the `event-publishing` delta in this change). Envelope-only validation left `detail.metadata.schemaVersion` an unverified assertion, so version-pinned subscription rules routed on an unearned claim.

**Migration**: None required. Publication of non-conformant payloads was never a guaranteed behavior; publishers already sending conformant events are unaffected. Publishers relying on non-conformant delivery will begin receiving synchronous `400 Bad Request` rejections.
