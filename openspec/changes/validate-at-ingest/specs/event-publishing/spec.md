## MODIFIED Requirements

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

## ADDED Requirements

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

The admission step SHALL cache the latest schema version and content per `(environment, schema)` in Lambda execution-environment memory with a TTL not exceeding 60 seconds. Registry reads SHALL occur only on a cold cache, on TTL expiry, or on the refresh-before-reject path — never unconditionally per publish. When a registry read is required and fails (including throttling), admission SHALL fail closed: the API SHALL return `503 Service Unavailable` and the event SHALL NOT be queued, preserving the invariant that every queued event was validated against a known schema.

#### Scenario: Hot path performs no registry call

- **WHEN** admission holds a fresh cached entry for the target `(environment, schema)`
- **THEN** resolution and validation SHALL use the cached content without any Schemas API call

#### Scenario: Cold cache fetches once

- **WHEN** admission has no cached entry for the target `(environment, schema)`
- **THEN** admission SHALL fetch the schema content once and populate the cache
- **AND THEN** subsequent publishes SHALL be served from the cache until expiry

#### Scenario: Registry failure fails closed

- **WHEN** admission requires a registry read and the Schemas API fails or throttles
- **THEN** the API SHALL return `503 Service Unavailable`
- **AND THEN** the event SHALL NOT be queued or stamped from an expired entry
