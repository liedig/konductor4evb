## MODIFIED Requirements

### Requirement: Admission schema cache

The admission step SHALL cache the latest business schema version and content in Lambda execution-environment memory with a TTL not exceeding 60 seconds. Cache identity SHALL include environment, schema, and the active metadata revision generation. Registry reads SHALL occur only on a cold cache, on TTL expiry, on a metadata-generation change that makes an entry cold, or on the refresh-before-reject path, never unconditionally per publish. When a registry read is required and fails (including throttling), admission SHALL fail closed with `503 Service Unavailable` and SHALL NOT queue the event.

The mutable environment metadata pointer SHALL be resolved authoritatively for each admission; it SHALL NOT use the business cache's TTL. Immutable metadata definitions and compiled composed validators MAY be cached by exact revision/version identity. A metadata-generation change SHALL force fresh current-business-schema resolution before using the new composition generation. Configuration and schema read failures SHALL NOT cause validation to fall back to an earlier contract.

#### Scenario: Hot path performs no registry call

- **WHEN** admission holds a fresh cached entry for the target environment, schema, and authoritatively resolved metadata generation
- **THEN** business resolution and validation SHALL use the cached content without any Schemas API call

#### Scenario: Cold cache fetches once

- **WHEN** admission has no cached entry for the target environment, schema, and metadata generation
- **THEN** admission SHALL fetch the business schema content once and populate the cache
- **AND THEN** subsequent publishes with that generation SHALL be served from the cache until expiry

#### Scenario: Registry failure fails closed

- **WHEN** admission requires a registry read and the Schemas API fails or throttles
- **THEN** the API SHALL return `503 Service Unavailable`
- **AND THEN** the event SHALL NOT be queued or stamped from an expired entry

#### Scenario: Warm instance observes metadata activation

- **WHEN** a warm instance's next authoritative lookup follows an M2 activation
- **THEN** it SHALL use M2 and resolve the current business schema for the M2 cache generation
- **AND THEN** it SHALL NOT reuse an M1 composed validator or label M1-generated values as M2

## ADDED Requirements

### Requirement: Generated metadata participates in admission validation

After existing publisher authorization and domain checks, admission SHALL resolve an active metadata revision, validate business data, generate metadata using trusted context, and validate generated fields against the exact composed contract. The reserved queue-time `publishedAt` value SHALL retain its existing semantics: admission SHALL validate its fixed generation rule and exact serialized-size contribution; delivery SHALL deterministically materialize and check the actual SQS timestamp. All other generated values SHALL be frozen before queueing.

Caller business-payload violations SHALL retain `422` semantics and refresh-before-reject behavior. Invalid stored metadata, unavailable required runtime context, or generator failures SHALL return `503`, not blame the producer with `422`, and SHALL never queue an event. Oversized final envelopes SHALL retain the admission-time `400` rejection, including all configured metadata in the size calculation.

#### Scenario: Valid composed event admitted

- **WHEN** business data and platform-generated metadata conform to their resolved contracts
- **THEN** the request SHALL be durably queued before returning `202`
- **AND THEN** the message SHALL contain the exact metadata revision and generated values needed for delivery

#### Scenario: Generator fails

- **WHEN** an active field requires trusted environment context that cannot be resolved or its generator produces an invalid value
- **THEN** admission SHALL return `503 Service Unavailable`
- **AND THEN** it SHALL NOT omit the field, use a producer-supplied value, or queue the request

#### Scenario: Metadata pushes the event over the size limit

- **WHEN** generated metadata makes the final EventBridge entry exceed its supported size
- **THEN** admission SHALL return `400 Bad Request` before queue acceptance
- **AND THEN** the oversized entry SHALL NOT be left for delivery retries

#### Scenario: Producer attempts to inject metadata

- **WHEN** a publish request supplies metadata or a metadata revision outside the supported request fields
- **THEN** the API SHALL reject the unsupported fields
- **AND THEN** no supplied extension value SHALL reach the delivered metadata

### Requirement: Delivery uses the admitted metadata contract

Delivery SHALL use only the admitted metadata revision, frozen values, and deterministic reserved queue-time fields. It SHALL NOT resolve the current metadata revision, regenerate extension values, or revalidate accepted events against a newer contract. Queue messages SHALL explicitly distinguish new-format admitted metadata from legacy messages; malformed new-format messages SHALL fail processing rather than silently become legacy messages.

#### Scenario: Delivery continues during configuration outage

- **WHEN** a valid new-format message is queued and the metadata configuration service later becomes unavailable
- **THEN** delivery SHALL remain possible without querying configuration
- **AND THEN** the event SHALL retain its admitted values and contract identity

#### Scenario: Malformed new-format queue message fails closed

- **WHEN** a new-format queue message lacks required admitted metadata or has inconsistent business and metadata version stamps
- **THEN** delivery SHALL report a processing failure for the existing retry/DLQ handling
- **AND THEN** it SHALL NOT regenerate the missing metadata or publish it under the legacy contract
