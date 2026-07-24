## MODIFIED Requirements

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

## ADDED Requirements

### Requirement: Publish API owns canonical envelope construction

The publish API SHALL accept only an event name and business `data` for the selected domain and environment. Nexus SHALL derive `source` from the domain's registered namespace identity, use the event name as `detail-type`, resolve the selected environment's target bus and latest schema version, and construct `detail` with a platform-owned `metadata` object and the caller's `data`. Callers SHALL NOT supply or override `source`, `detail-type`, target bus, or `detail.metadata.schemaVersion`.

#### Scenario: Canonical envelope is derived

- **WHEN** a publisher sends event `OrderNotification` with business data to the `orders` domain in `prod`
- **THEN** Nexus SHALL publish with `source` equal to `prezzee-orders`
- **AND THEN** `detail-type` SHALL equal `OrderNotification`
- **AND THEN** the target SHALL be the platform-resolved `orders` prod bus

#### Scenario: Latest schema version is stamped

- **WHEN** the latest selected-environment version of `prezzee-orders@OrderNotification` is 4
- **THEN** Nexus SHALL publish `detail.metadata.schemaVersion` as numeric value `4`
- **AND THEN** `detail.data` SHALL contain the caller's business data

### Requirement: Publish-time validation is envelope-only

Before calling EventBridge, the publish API SHALL validate the complete generated envelope, including canonical identity, numeric schema version, required `metadata`/`data` structure, serializability, and EventBridge entry constraints. It SHALL NOT validate the business `data` against the schema document.

#### Scenario: Business data is not schema-validated

- **WHEN** an authorized publisher supplies JSON business data that does not satisfy the registered schema
- **THEN** Nexus SHALL NOT reject the request on schema-conformance grounds
- **AND THEN** Nexus SHALL continue envelope validation and publication

#### Scenario: Invalid generated envelope fails before delivery

- **WHEN** the complete generated envelope violates a required envelope or EventBridge entry constraint
- **THEN** the API SHALL reject the request with a validation error
- **AND THEN** Nexus SHALL NOT call EventBridge for that event
