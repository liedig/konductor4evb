# Capability: Event Envelope

## ADDED Requirements

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

Event names (`detail-type` values) SHALL satisfy the constraints their derived resource names inherit: characters limited to `[A-Za-z0-9._-]` and a combined rule name `{consumerDomain}-{detailType}` within EventBridge's 64-character limit. Until schema registration moves in-platform, these constraints SHALL be enforced at subscribe time; when registration is in-platform, enforcement SHALL move to registration time.

#### Scenario: Constraint enforced at subscribe time in v1

- **WHEN** a consumer subscribes to a schema whose event name contains a space or pushes the rule name over 64 characters
- **THEN** the subscribe command SHALL fail validation with an error describing the constraint the event name violates
