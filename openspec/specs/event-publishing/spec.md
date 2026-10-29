# Capability: Event Publishing

## Purpose

How producers publish events to a domain without knowing EventBridge internals: a flat store-and-forward API that admits requests synchronously (authorization, payload conformance validation against the latest schema, version stamping), queues only admitted events durably, stamps the canonical envelope with immutable publish metadata, and delivers to the domain's event bus with retries.

## Requirements

### Requirement: Flat store-and-forward publish API

The separate OAuth and IAM control-plane surfaces SHALL each provide `POST /publish` as a flat verb endpoint where the target domain is in the request body, not the URL path. The request SHALL accept `domain`, `eventName`, `data`, and an optional `idempotencyKey`. The API SHALL invoke a synchronous admission step before queueing: admission performs authorization, schema resolution, payload conformance validation, and version stamping, and only admitted requests SHALL be queued to SQS for store-and-forward durability. The API SHALL return `202 Accepted` with the SQS message ID as `requestId` only after queue acceptance. Admission rejections (authorization, validation, missing schema) SHALL return a synchronous `4xx` identifying the failure, and the event SHALL NOT be queued; the caller owns the event until `202` and SHALL treat any non-`202` as not accepted. The publish consumer Lambda SHALL read the admitted message, construct the canonical envelope from the admitted attributes, and call EventBridge `PutEvents`. If `PutEvents` fails, SQS SHALL retry with the same message attributes, preserving the idempotency key and published timestamp.

#### Scenario: Event queued for durable delivery

- **WHEN** an authorized publisher sends a publish request whose payload conforms to the latest schema
- **THEN** the API SHALL return `202 Accepted` with the SQS message ID as `requestId`
- **AND THEN** the event SHALL be durably queued for delivery

#### Scenario: PutEvents failure triggers SQS retry

- **WHEN** the publish consumer calls `PutEvents` and EventBridge rejects the entry
- **THEN** the SQS message SHALL NOT be deleted
- **AND THEN** SQS SHALL retry delivery with the same message attributes, idempotency key, and published timestamp

#### Scenario: Event is never lost after queue acceptance

- **WHEN** a publish request has been accepted by SQS
- **THEN** the event SHALL survive downstream failures and be retried until delivery or DLQ exhaustion

#### Scenario: Rejected admission is synchronous and never queued

- **WHEN** a publish request fails admission (unauthorized, non-conformant payload, missing schema)
- **THEN** the API SHALL return a synchronous `4xx` identifying the failure
- **AND THEN** no SQS message SHALL be queued and no EventBridge call SHALL occur

#### Scenario: Publisher owns the event until acceptance

- **WHEN** the API returns any non-`202` response, including `5xx` during an admission outage
- **THEN** the event SHALL NOT be durably queued
- **AND THEN** redelivery SHALL be the publisher's responsibility until a `202` is received

### Requirement: Environment inferred from API stage

The publish and schema write APIs SHALL infer the target environment from the API Gateway stage name (`$context.stage`). Stages SHALL be named after platform environments (`dev`, `staging`, `prod`). The admission step SHALL read the stage from the API Gateway request context and SHALL inject it as an SQS message attribute named `Environment` on the admitted message. The publish consumer Lambda SHALL read the `Environment` attribute to select the environment-specific event bus. The caller SHALL NOT specify an environment field in the request body.

#### Scenario: Stage determines target environment

- **WHEN** a publisher sends `POST /publish` to the `prod` stage for domain `orders`
- **THEN** admission SHALL resolve schemas from `{org}-prod-orders-schema-registry`
- **AND THEN** the consumer SHALL publish to `{org}-prod-orders-bus`

#### Scenario: Same operation on different stages targets different environments

- **WHEN** a publisher sends `POST /publish` to the `dev` stage and then to the `prod` stage for the same domain and event
- **THEN** each invocation SHALL target the respective environment's registry and bus independently

#### Scenario: Environment-agnostic operations ignore the stage

- **WHEN** a caller invokes register, approve, list, or describe operations on any stage
- **THEN** the stage SHALL NOT affect the operation's behavior

### Requirement: Publisher resource authorization

At admission, before accessing domain or schema metadata or queueing, Konductor SHALL submit the normalized principal, publisher action, and target domain to the DynamoDB-backed grant engine. A `publisher` resource-role binding MAY bind an issuer-qualified Cognito team group or AWS-account principal to a domain, and a principal MAY have bindings for one or more domains. The `publisher` role SHALL NOT be interpreted as a global Cognito role, and a binding for one domain SHALL NOT authorize another domain. Authorization denials SHALL return a synchronous `403 Forbidden` and SHALL NOT queue the event.

#### Scenario: Team group binding authorizes its domain

- **WHEN** the caller belongs to a Cognito team group with a `publisher` binding for domain `orders`
- **THEN** the caller SHALL be authorized to publish events for `orders`

#### Scenario: AWS account binding authorizes its domain

- **WHEN** an IAM caller's normalized AWS account has a `publisher` binding for domain `orders`
- **THEN** the caller SHALL be authorized to publish events for `orders` through the IAM control-plane surface

#### Scenario: Binding is domain-isolated

- **WHEN** the caller has a `publisher` binding for `orders` but requests publication for `payments`
- **THEN** admission SHALL deny authorization with `403 Forbidden`
- **AND THEN** admission SHALL NOT resolve the schema, queue the event, or call EventBridge

#### Scenario: Global publisher claim does not confer access

- **WHEN** a caller has a Cognito claim or group name containing `publisher` but no Konductor `publisher` binding for the target domain
- **THEN** admission SHALL deny authorization with `403 Forbidden`

### Requirement: Latest stage-selected-environment schema resolution

For every publish request, the admission step SHALL derive the canonical schema name from the active domain namespace and event name and resolve the latest numeric schema version in the stage-selected environment's platform-managed registry, served from the admission schema cache rather than a per-request registry call. Admission SHALL use that resolved version for envelope stamping, and the stamped version SHALL always equal the version the payload was validated against. The caller SHALL NOT choose a schema name or schema version.

#### Scenario: Stage-selected environment controls version resolution

- **WHEN** event `OrderNotification` has latest version 5 in `dev` and version 3 in `prod`, and the caller publishes to the `prod` stage
- **THEN** admission SHALL resolve the schema from the prod registry
- **AND THEN** the admitted message SHALL carry numeric schema version `3` for envelope stamping

#### Scenario: Schema is absent from stage-selected environment

- **WHEN** the derived schema does not exist in the stage-selected environment's registry
- **THEN** admission SHALL return `404 Not Found` identifying the schema
- **AND THEN** no SQS message SHALL be queued and no EventBridge call SHALL occur

#### Scenario: Caller-supplied version is not accepted

- **WHEN** a publish request attempts to supply a schema name or schema version
- **THEN** the API SHALL reject the unsupported field
- **AND THEN** Konductor SHALL resolve identity and version from platform state

### Requirement: Payload conformance validation at admission

For every publish request, the admission step SHALL validate the request `data` against the latest-version schema document of the derived schema in the stage-selected environment's registry. A non-conformant payload SHALL be rejected with `422 Unprocessable Entity` carrying human-readable, instance-located validation violations, and SHALL NOT be queued. Before rejecting on conformance grounds, admission SHALL refresh its cached schema from the registry once and re-validate, so a conformance rejection always reflects the true latest registered version rather than a stale cache entry. Every stored schema is JSON Schema draft-4 (schema management rejects anything else at write time); a stored document that nonetheless cannot be compiled — including one declaring a newer `$schema` dialect — SHALL make admission fail closed with `503 Service Unavailable`, never skip validation.

#### Scenario: Conformant payload admitted

- **WHEN** an authorized publisher sends a payload that conforms to the latest registered schema version
- **THEN** the request SHALL be admitted and queued
- **AND THEN** the admitted schema version SHALL equal the version the payload was validated against

#### Scenario: Non-conformant payload rejected with violations

- **WHEN** an authorized publisher sends a payload that fails the latest registered schema
- **THEN** the API SHALL return `422 Unprocessable Entity` with validation violations identifying the failing instance paths
- **AND THEN** no SQS message SHALL be queued and no EventBridge call SHALL occur

#### Scenario: Rejection reflects the true latest version

- **WHEN** admission's cached schema fails a payload, but a registry refresh reveals a newer version
- **THEN** admission SHALL re-validate against the refreshed schema before deciding
- **AND THEN** a payload conforming to the refreshed version SHALL be admitted rather than rejected against the stale entry

#### Scenario: Unusable stored schema fails closed

- **WHEN** the stored schema content cannot be compiled as JSON Schema draft-4 (malformed, or declaring a newer `$schema` dialect)
- **THEN** the API SHALL return `503 Service Unavailable`
- **AND THEN** the event SHALL NOT be queued, and no publish SHALL bypass conformance validation

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

### Requirement: Publisher permissions exclude platform administration

The `publisher` role SHALL authorize only publisher schema operations and event publication for bound domains. It SHALL NOT authorize management of registries, event buses, resource policies, EventBridge rules, subscriber grants, publisher bindings, or platform authorization configuration.

#### Scenario: Publisher attempts an administrative operation

- **WHEN** a caller has only a `publisher` binding for the target domain
- **THEN** Konductor SHALL deny registry, bus, policy, rule, subscriber-grant, and authorization-administration operations

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

### Requirement: Python SDK publication preserves business data

The Python SDK SHALL expose `publish` with required domain, event name, and data arguments and an optional idempotency key. It SHALL send the existing publish request without inventing envelope metadata or allowing a caller-selected schema version. Data SHALL support all JSON value kinds without coercing values or modifying the caller's input.

#### Scenario: JSON values retain their meaning
- **WHEN** a caller publishes an object, array, string, integer, finite floating-point number, boolean, or explicit null, including nested values and Unicode
- **THEN** the transmitted `data` SHALL preserve the JSON value and its value kind
- **AND** boolean values SHALL NOT become integers, integers SHALL NOT be coerced through floating-point representation, and null SHALL NOT be omitted

#### Scenario: Missing and explicit-null data differ
- **WHEN** a caller omits the required data argument
- **THEN** the SDK SHALL reject the call locally
- **AND WHEN** the caller explicitly supplies null
- **THEN** the SDK SHALL include `"data": null` and leave business-schema conformance to admission

#### Scenario: Invalid JSON input
- **WHEN** a payload includes a non-finite number, non-string object key, or unsupported Python object
- **THEN** the SDK SHALL reject it locally without stringifying it, sending invalid JSON, or making a network request

### Requirement: Python publish returns admission acceptance

A successful Python publish call SHALL return an acceptance receipt containing the server's queue request identifier and available message. It SHALL NOT report EventBridge delivery, subscriber receipt, or a client-generated event identifier as the outcome. Synchronous admission failures SHALL remain service errors.

#### Scenario: Valid publication accepted
- **WHEN** admission returns HTTP 202 with a nonempty `requestId`
- **THEN** the SDK SHALL return that identifier unchanged in an acceptance receipt
- **AND** it SHALL NOT wait for downstream event delivery

#### Scenario: Payload rejected by the schema
- **WHEN** admission returns HTTP 422 with payload violations
- **THEN** the SDK SHALL raise a service error preserving the violations
- **AND** it SHALL NOT return an acceptance receipt or retry the request

### Requirement: Python publication does not imply cross-request deduplication

The Python SDK SHALL forward a supplied idempotency key unchanged and omit it when absent, preserving server defaulting. It SHALL NOT automatically retry publication or describe a repeated key as duplicate suppression. Documentation SHALL distinguish retrying delivery of one admitted event from submitting another publish request.

#### Scenario: Supplied and omitted keys
- **WHEN** a caller supplies an idempotency key
- **THEN** the SDK SHALL send that key unchanged
- **AND WHEN** the caller omits it
- **THEN** the SDK SHALL leave key generation to admission rather than invent a new SDK default

#### Scenario: Caller deliberately resubmits
- **WHEN** the caller invokes publish again with the same key after losing a response
- **THEN** the SDK SHALL submit a new request only because the caller explicitly invoked it
- **AND** documentation SHALL explain that duplicate events and different admission-generated metadata remain possible
