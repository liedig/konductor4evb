# Capability: Schema Management

## Purpose

How event schemas are discovered and managed: read-only discovery across each domain's per-environment registries, plus publisher-authorized schema creation and append-only version registration in platform-managed registries, with the target environment inferred from the API stage.

## Requirements

### Requirement: Schema discovery

The control plane SHALL provide read-only schema discovery as an entitlement-filtered consumer catalog. `GET /schemas?consumerDomain={consumerDomain}` SHALL list exactly the schemas covered by the consumer domain's current entitlements, expanded across environments from the per-environment registries (source of truth), deduplicating overlapping pattern coverage, and reporting the latest version per environment. Producer-domain and environment filters MAY narrow the expansion, and pagination SHALL use an opaque continuation token carrying cursors only, never authorization state. `GET /schemas/{schemaName}?consumerDomain={consumerDomain}&domain={producerDomain}` SHALL return schema detail including per-environment latest versions and the latest content under the same coverage check. Both routes SHALL require the caller to hold a `subscriber` binding on the queried consumer domain. `platform-admin` and `platform-auditor` callers MAY omit `consumerDomain` and receive the platform-wide catalog unfiltered by entitlements. No separate schema or catalog table SHALL be introduced.

A caller holding a `publisher` binding on a producer domain SHALL be able to view that domain's own catalog without any consumer entitlement: `GET /schemas?domain={producerDomain}` (no `consumerDomain`) SHALL return the producer domain's schemas with the latest version per environment, and `GET /schemas/{schemaName}?domain={producerDomain}` SHALL return detail, under the same registry-backed expansion. The producer scope SHALL NOT expose other domains' schemas and SHALL NOT require or consult entitlements.

#### Scenario: Catalog expands current entitlements

- **WHEN** a caller with a `subscriber` binding on `checkout` requests `GET /schemas?consumerDomain=checkout`
- **THEN** the response SHALL list exactly the schemas covered by `checkout`'s current entitlements, keyed by schema name, with the latest version per environment

#### Scenario: Publisher views their own domain's schemas

- **WHEN** a caller with a `publisher` binding on `orders` requests `GET /schemas?domain=orders` without `consumerDomain`
- **THEN** the response SHALL list the schemas registered for `orders` with the latest version per environment
- **AND THEN** no entitlement SHALL be required or consulted

#### Scenario: Publisher scope does not widen to other domains

- **WHEN** a caller with a `publisher` binding on `orders` requests `GET /schemas?domain=payments` without `consumerDomain` and holds no binding on `payments`
- **THEN** the API SHALL return `403 Forbidden`

#### Scenario: Version skew visible before subscribing

- **WHEN** a covered schema's latest version differs across environments (e.g., dev: 5, staging: 3, prod: 3)
- **THEN** the catalog response SHALL report each environment's latest version distinctly, so consumers see skew before signing the contract

#### Scenario: List a domain's schemas

- **WHEN** a caller with a `subscriber` binding on `checkout` requests `GET /schemas?consumerDomain=checkout&domain=orders`
- **THEN** the response SHALL list each `orders` schema covered by `checkout`'s current entitlements, keyed by schema name, with the latest version per environment

#### Scenario: Missing schema or domain

- **WHEN** a caller requests catalog detail for a schema name absent from every environment
- **THEN** the API SHALL return `404 Not Found` identifying the missing schema
- **AND THEN** a catalog list narrowed to a producer domain that does not exist or is not `ACTIVE` SHALL return an empty list rather than `404 Not Found`

#### Scenario: Zero entitlements yields an empty catalog

- **WHEN** a caller with a `subscriber` binding requests the catalog for a consumer domain with no current entitlements
- **THEN** the response SHALL be an empty list

#### Scenario: Uncovered detail returns 403

- **WHEN** a caller requests catalog detail for a schema not covered by the queried consumer domain's current entitlements
- **THEN** the API SHALL return `403 Forbidden`
- **AND THEN** a request for a schema absent from every environment SHALL return `404 Not Found`

#### Scenario: Missing consumer domain binding denied

- **WHEN** a caller without a `subscriber` binding on the queried consumer domain requests the catalog
- **THEN** the API SHALL return `403 Forbidden`

#### Scenario: Platform roles inspect the full catalog

- **WHEN** a `platform-admin` or `platform-auditor` caller requests `GET /schemas` without `consumerDomain` and without `domain`
- **THEN** the response SHALL span all discoverable schemas unfiltered by entitlements
- **AND THEN** `platform-auditor` SHALL NOT be able to perform any mutation

#### Scenario: Pages follow a stable ordering without duplicates

- **WHEN** a caller pages through the catalog with a continuation token
- **THEN** entries SHALL be ordered by producer domain and schema name
- **AND THEN** no schema SHALL appear twice within the page sequence

#### Scenario: Revoked coverage never appears in later pages

- **WHEN** a caller has retrieved a catalog page
- **AND WHEN** an entitlement covering a previously listed schema is revoked
- **THEN** the next page request with the continuation token SHALL NOT contain that schema

#### Scenario: Mid-sequence grants do not appear behind the watermark

- **WHEN** a caller is paging through the catalog
- **AND WHEN** a new entitlement is granted covering schemas that sort at or before the current position
- **THEN** those schemas SHALL NOT appear in the remaining pages of the sequence
- **AND THEN** a fresh catalog request SHALL include them

#### Scenario: Cursor rejected when filters change

- **WHEN** a caller presents a continuation token with different producer-domain or environment filters than the request that issued it
- **THEN** the API SHALL return `400 Bad Request`

### Requirement: Flat publisher schema creation API

The separate OAuth and IAM control-plane surfaces SHALL each provide `POST /schemas` as a flat verb endpoint for creating an event schema. The request SHALL accept `domain`, `eventName`, `schemaType`, and `content` in the request body. The target environment SHALL be inferred from the API Gateway stage (`$context.stage`). Both surfaces SHALL normalize their authenticated principal and require the DynamoDB-backed grant engine to allow the publisher action for that domain. Konductor SHALL derive the canonical schema name `{namespaceIdentity}@{eventName}` and the stage-selected environment's registry. Both route contracts SHALL reject a duplicate schema rather than replacing it. The `schemaType` SHALL be `JSONSchemaDraft4` and the `content` SHALL parse as a valid JSON Schema draft-4 document: admission-time payload validation requires an unambiguous draft-4 document, and `OpenApi3` content (whose payload shape selection is conventional and whose dialect differs from draft 4) SHALL be rejected with `400 Bad Request`.

#### Scenario: Authorized publisher creates a schema

- **WHEN** a caller with a `publisher` binding for domain `orders` creates event `OrderNotification` on the `dev` stage
- **THEN** Konductor SHALL create schema `{namespaceIdentity}@OrderNotification` in registry `{org}-dev-orders-schema-registry`
- **AND THEN** the schema SHALL be created as version 1
- **AND THEN** the API SHALL return `201 Created` with the schema name and version

#### Scenario: OpenAPI content rejected at creation

- **WHEN** a publisher requests schema creation with `schemaType: OpenApi3`
- **THEN** the API SHALL return `400 Bad Request` identifying `JSONSchemaDraft4` as the supported type
- **AND THEN** no schema SHALL be created

#### Scenario: Malformed schema content rejected at creation

- **WHEN** a publisher requests schema creation with `content` that does not parse as a valid JSON Schema draft-4 document
- **THEN** the API SHALL return `400 Bad Request` identifying the parse failure
- **AND THEN** no schema SHALL be created

#### Scenario: Caller lacks a binding for the target domain

- **WHEN** an authenticated caller without a `publisher` binding for domain `orders` requests schema creation for `orders`
- **THEN** the API SHALL return `403 Forbidden`
- **AND THEN** no schema SHALL be created

#### Scenario: Existing schema is not replaced

- **WHEN** a publisher requests creation of an event whose canonical schema name already exists in the stage-selected registry
- **THEN** the API SHALL return `409 Conflict`
- **AND THEN** the existing schema and all of its versions SHALL remain unchanged

#### Scenario: Inactive or missing domain

- **WHEN** a publisher requests schema creation for a domain that does not exist or is not `ACTIVE`
- **THEN** the API SHALL return `404 Not Found`
- **AND THEN** no registry write SHALL occur

### Requirement: Flat append-only schema version API

The separate OAuth and IAM control-plane surfaces SHALL each provide `POST /schemas/versions` as a flat verb endpoint for appending a schema version. The request SHALL accept `domain`, `eventName`, and `content` in the request body. The target environment SHALL be inferred from the API Gateway stage. Both surfaces SHALL normalize their authenticated principal and require the grant engine to allow the publisher action for that domain. Konductor SHALL derive the canonical schema name and SHALL use the AWS Schemas version-producing operation so existing versions remain immutable. The appended `content` SHALL parse as a valid JSON Schema draft-4 document, and SHALL be rejected with `400 Bad Request` otherwise.

The appended `content` SHALL be backward compatible with the current latest version in the stage-selected registry: every payload valid under the current latest version SHALL remain valid under the appended version. Konductor SHALL enforce this with syntactic additive-only rules, rejecting an append with `400 Bad Request` identifying the offending paths when it removes a declared property, newly requires a property, narrows a type, or tightens a constraint (including raising `minimum`, lowering `maximum`, shrinking `enum`, or flipping `additionalProperties` to `false`). Appends that add optional properties, drop `required` entries, widen types, or loosen or remove constraints SHALL be accepted. Compatibility SHALL be judged against the current latest version only, never against historical versions. The gate SHALL be absolute: no request flag, role, or binding SHALL bypass it, and intentional breaking evolution SHALL be expressed as a new schema under a new event name, never as a new version. Schema creation (`POST /schemas`) SHALL NOT be gated — version 1 has no predecessor. Publisher APIs SHALL NOT expose schema deletion, retirement, version deletion, or historical-version replacement.

#### Scenario: Authorized publisher appends a version

- **WHEN** a caller with a `publisher` binding for domain `orders` appends a schema document for event `OrderNotification` on the `staging` stage
- **THEN** Konductor SHALL create the next version of `{namespaceIdentity}@OrderNotification` in `{org}-staging-orders-schema-registry`
- **AND THEN** all earlier versions SHALL remain unchanged
- **AND THEN** the API SHALL return `201 Created` with the schema name and new version

#### Scenario: Additive append is accepted

- **WHEN** a publisher appends a document that adds an optional property, drops a `required` entry, or widens a type relative to the current latest version
- **THEN** the API SHALL return `201 Created` with the new version

#### Scenario: Tightening append is rejected

- **WHEN** a publisher appends a document that newly requires a property the current latest version treats as optional
- **THEN** the API SHALL return `400 Bad Request` identifying the offending path
- **AND THEN** no new version SHALL be created

#### Scenario: Constraint tightening is rejected

- **WHEN** a publisher appends a document that raises a `minimum`, lowers a `maximum`, shrinks an `enum`, or flips `additionalProperties` to `false` relative to the current latest version
- **THEN** the API SHALL return `400 Bad Request` identifying the offending path
- **AND THEN** no new version SHALL be created

#### Scenario: Removed property is rejected

- **WHEN** a publisher appends a document that omits a property declared by the current latest version
- **THEN** the API SHALL return `400 Bad Request` identifying the offending path
- **AND THEN** no new version SHALL be created

#### Scenario: Compatibility is judged against the latest version only

- **WHEN** the current latest version is v4 and a publisher appends a document compatible with v3 but tightening relative to v4
- **THEN** the API SHALL return `400 Bad Request`
- **AND THEN** compatibility with earlier historical versions SHALL NOT rescue the append

#### Scenario: The gate admits no override

- **WHEN** any caller — including one holding a `platform-admin` role — appends a tightening document
- **THEN** the API SHALL return `400 Bad Request`
- **AND THEN** no request flag SHALL exist that forces acceptance

#### Scenario: Malformed version content rejected

- **WHEN** a publisher appends content that does not parse as a valid JSON Schema draft-4 document
- **THEN** the API SHALL return `400 Bad Request` identifying the parse failure
- **AND THEN** no new version SHALL be created

#### Scenario: Version target does not exist

- **WHEN** a publisher attempts to append a version for an event whose canonical schema does not exist in the stage-selected registry
- **THEN** the API SHALL return `404 Not Found`
- **AND THEN** no schema SHALL be created implicitly

#### Scenario: Publisher mutation surface is append-only

- **WHEN** a publisher inspects or invokes the schema-management API
- **THEN** no publisher operation SHALL exist to delete or retire a schema, delete a version, or replace a historical version

### Requirement: Publisher schema operations do not grant registry administration

Publisher authorization SHALL permit only schema creation and version appending inside registries of bound domains. It SHALL NOT permit publishers to create, update, or delete registries or to administer registry policies. Registry lifecycle and policy administration SHALL remain platform-admin operations.

#### Scenario: Publisher attempts registry administration

- **WHEN** a caller has a `publisher` binding for a domain
- **THEN** that binding SHALL NOT authorize any registry lifecycle or registry policy operation

#### Scenario: One binding covers multiple environments

- **WHEN** a caller's team group has a `publisher` binding for domain `orders`
- **THEN** the caller MAY use publisher schema operations on any environment stage for `orders`

#### Scenario: AWS account binding uses identical schema semantics

- **WHEN** an IAM caller's normalized AWS account has a `publisher` binding for domain `orders`
- **THEN** the IAM surface SHALL authorize the same schema operations and enforce the same domain constraints as the OAuth surface

### Requirement: Schema discovery is read-only

The schema-management capability SHALL provide schema creation and append-only version registration only through the publisher-authorized operations defined by `add-publisher-apis`. It SHALL NOT provide publisher operations for schema replacement, retirement, version deletion, schema deletion, registry lifecycle, or registry policy administration. Discovery SHALL be filtered by the caller-visible consumer domain's current entitlements; the interim baseline in which any authenticated platform caller could discover any active domain's schemas SHALL NOT remain available.

#### Scenario: Only append-only write operations exposed

- **WHEN** a publisher uses the Konductor schema API
- **THEN** the only schema write operations exposed SHALL be schema creation and appending a new schema version

#### Scenario: Global visibility baseline is retired

- **WHEN** any caller requests schema discovery without an entitlement-covered consumer domain or a platform role
- **THEN** the response SHALL contain no schemas
- **AND THEN** the pre-existing behavior of serving all active domains' schemas to any authenticated caller SHALL NOT remain available

### Requirement: Schema version publication events

The control plane SHALL emit a `SchemaVersionPublished` event on the control-plane bus (source `konductor.schema-management`) after every successful schema create or append, carrying `producerDomain`, `schemaName`, `environment` (the stage-selected environment whose registry was written), `version`, and `publishedAt`. This event is the platform's promotion signal: it is how consumers' subscriptions learn that a schema has arrived in a new environment. Emission follows the committed registry write; a failed emission SHALL be logged and SHALL NOT fail the API response, since the registry write is the committed state and appends are not safely retryable (a retry would mint a further version).

#### Scenario: Create emits version 1 for the target environment

- **WHEN** a publisher successfully registers `OrderNotification` against the staging environment
- **THEN** a `SchemaVersionPublished` event SHALL be emitted with `environment: staging` and `version: 1`

#### Scenario: Append emits the new version

- **WHEN** a publisher successfully appends version 5 of a schema in prod
- **THEN** a `SchemaVersionPublished` event SHALL be emitted with `environment: prod` and `version: 5`

#### Scenario: Emission failure does not fail the write

- **WHEN** the registry write has succeeded but emitting the event fails
- **THEN** the API response SHALL report the write's success
- **AND THEN** the emission failure SHALL be logged with the schema identity and environment

### Requirement: Platform-managed metadata revision configuration

Konductor SHALL expose organisation-level metadata configuration through platform administration on the OAuth surface. Only `platform-admin` SHALL create revisions or activate them; `platform-auditor` SHALL have read-only access. Domain publisher/subscriber bindings SHALL NOT authorize configuration management. Definitions SHALL support fields and nested objects generated by a bounded supported catalogue including UUIDs, UTC dates/timestamps, AWS region, organisation identifier, and environment. Types and validation constraints SHALL be consistent with the generator. Arbitrary scripts, executable expressions, external lookups, and remote schema references SHALL NOT be accepted. Revision size, depth, and field count SHALL be bounded with documented limits.

#### Scenario: Administrator creates a valid revision

- **WHEN** a platform administrator submits a valid extension definition against the latest configuration revision
- **THEN** Konductor SHALL persist an immutable new revision with its predecessor, definition, author, and creation time
- **AND THEN** creating the revision SHALL NOT activate it in any environment

#### Scenario: Non-administrator attempts a mutation

- **WHEN** an auditor or caller holding only domain bindings attempts to save or activate metadata configuration
- **THEN** the API SHALL deny the operation with `403 Forbidden`
- **AND THEN** no configuration state SHALL change

#### Scenario: Unsupported generation rejected

- **WHEN** a definition contains an unknown generator, incompatible type, external reference, reserved name, or exceeds a configured bound
- **THEN** the API SHALL reject it with `400 Bad Request` identifying the invalid path
- **AND THEN** no revision SHALL be created

### Requirement: Additive immutable metadata revisions

A new metadata revision SHALL retain every existing field definition, type, generator, and constraint unchanged and SHALL only add fields or new children to extensible objects. The candidate SHALL be compared to the latest saved organisation-level revision. Configured fields SHALL be generated and required in that revision's exact validation contract; an old event SHALL continue to use its earlier contract rather than being required to acquire the new fields. No role or request flag SHALL bypass these rules. Concurrent saves based on an outdated predecessor SHALL conflict rather than overwrite or fork the revision history.

#### Scenario: New generated field is accepted

- **WHEN** an administrator adds `eventId` with a supported UUID generator without modifying existing fields
- **THEN** a new immutable revision SHALL be created
- **AND THEN** its exact contract SHALL require and validate `eventId`

#### Scenario: Existing field mutation rejected

- **WHEN** a candidate removes a field or changes its type, generator, or validation constraint
- **THEN** the candidate SHALL be rejected with the offending path
- **AND THEN** no force-accept operation SHALL be available

#### Scenario: Concurrent revision creation conflicts

- **WHEN** two administrators save against M2 and one has already committed M3
- **THEN** the other save SHALL return `409 Conflict`
- **AND THEN** the losing administrator SHALL be able to reload and review changes without losing their local edits

### Requirement: Environment-scoped metadata activation

A saved revision SHALL be activated explicitly per configured environment, applying to all domains in that environment with no per-domain override. Activation SHALL check supported generation, composition readiness, and resolved environment context before committing. It SHALL require the expected current revision and reject stale or backward transitions with `409 Conflict`; reapplying the already active revision SHALL be idempotent. Activation SHALL record the actor, environment, previous and new revisions, and activation time durably.

The activation boundary SHALL be the committed active-revision change. An admission whose authoritative revision lookup occurs after that commit SHALL use the new revision. An admission that selected the old revision before the commit MAY complete under it, even if its queue acceptance or response occurs later. Configuration outages SHALL NOT cause fallback to an older revision.

#### Scenario: Dev changes without changing prod

- **WHEN** an administrator activates M2 for dev while prod uses M1
- **THEN** subsequent dev revision lookups SHALL resolve M2 across domains
- **AND THEN** prod SHALL continue resolving M1

#### Scenario: Readiness failure leaves the environment unchanged

- **WHEN** a candidate cannot generate valid metadata using the target environment context or required readiness checks fail
- **THEN** activation SHALL fail without moving the active revision
- **AND THEN** the response SHALL identify the failure rather than report partial activation

#### Scenario: Competing activations cannot overwrite each other

- **WHEN** activation was requested against M1 but another request has already activated M2
- **THEN** a request for a different revision based on M1 SHALL return `409 Conflict`
- **AND THEN** it SHALL NOT overwrite M2

#### Scenario: Metadata activation does not promote a business schema

- **WHEN** M2 is activated
- **THEN** business versions and subscription pins SHALL remain unchanged
- **AND THEN** activation SHALL NOT emit `SchemaVersionPublished` or bypass the business-schema compatibility gate

### Requirement: Composed current and historical contract discovery

Authorized schema readers SHALL be able to retrieve the effective contract containing validated `metadata` and `data` sections, its exact business version and metadata revision, and the metadata generator descriptions. Existing raw business-schema responses and publisher authoring contracts SHALL remain available and unchanged. Both API surfaces SHALL preserve existing publisher, subscriber-entitlement, and platform-role visibility rules for composed reads.

Current contract discovery SHALL combine the current business version in the selected environment with its active metadata revision. Historical composition SHALL require explicit business version and metadata revision, with a documented legacy revision for events predating revision stamping. A historical business version alone SHALL NOT be silently composed with the active metadata revision. Exact documents SHALL resolve without external network access, preserving the business schema's local-reference semantics and original validation behavior.

#### Scenario: Current schema inherits activated metadata

- **WHEN** `OrderPlaced` is currently V3 and M2 is active in staging
- **THEN** current contract discovery for staging SHALL return a contract validating V3 business data and M2 metadata
- **AND THEN** the underlying V3 business schema SHALL not be rewritten

#### Scenario: Historical lookup remains reproducible

- **WHEN** an authorized caller requests the explicit V2/M1 contract after V3/M2 becomes current
- **THEN** the returned contract SHALL retain the V2/M1 validation rules
- **AND THEN** omission of M2-only fields SHALL not invalidate a V2/M1 event

#### Scenario: Historical revision is not guessed

- **WHEN** a caller requests historical composition with a business version but no metadata revision
- **THEN** the API SHALL return `400 Bad Request` explaining the required metadata selector

#### Scenario: Composition preserves business references

- **WHEN** a business schema uses local definitions and references
- **THEN** the composed document SHALL validate its `data` with equivalent business rules
- **AND THEN** validation SHALL not access remote resources or reinterpret a business root reference as a reference to the envelope

#### Scenario: Composition does not widen catalog access

- **WHEN** a caller lacks the required producer binding, consumer coverage, or platform role for a schema
- **THEN** requesting its composed or historical contract SHALL NOT reveal its content

### Requirement: Metadata lifecycle audit durability

Metadata revision creation and environment activation SHALL have durable audit records associated with their committed transitions. Downstream lifecycle notifications SHALL be driven from committed state and tolerate duplicate delivery without repeating a configuration mutation. Notification failure SHALL NOT undo or ambiguously fail a successful configuration commit.

#### Scenario: Notification delivery fails after activation

- **WHEN** activation commits but downstream lifecycle emission fails
- **THEN** the active revision and audit record SHALL remain committed
- **AND THEN** emission SHALL be retried from durable state rather than requiring the administrator to repeat activation

### Requirement: Business authoring preserves offline composition

Business schema creation and append SHALL validate that the authored document can participate in an offline composed contract without changing its business validation semantics. Unsupported or externally unresolved references SHALL be rejected before registration with `400 Bad Request` identifying the problem. This check SHALL NOT replace or weaken the independent business-schema compatibility gate. Publishers SHALL continue to submit only business schema content, not metadata definitions.

#### Scenario: Local references survive authoring

- **WHEN** a publisher authors a supported business schema containing local references
- **THEN** its composed representation SHALL preserve those references' original scope
- **AND THEN** the stored business document SHALL remain the submitted document

#### Scenario: Unresolvable reference is rejected before writing

- **WHEN** a proposed business schema depends on a reference that cannot be resolved offline in its composed contract
- **THEN** registration SHALL fail with `400 Bad Request`
- **AND THEN** no registry write or remote reference fetch SHALL occur
