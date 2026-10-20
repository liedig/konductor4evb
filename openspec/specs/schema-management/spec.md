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

The separate OAuth and IAM control-plane surfaces SHALL each provide `POST /schemas/versions` as a flat verb endpoint for appending a schema version. The request SHALL accept `domain`, `eventName`, and `content` in the request body. The target environment SHALL be inferred from the API Gateway stage, and appends SHALL be accepted only on the authoring environment's stage (see Schema authoring is restricted to the authoring environment). Both surfaces SHALL normalize their authenticated principal and require the grant engine to allow the publisher action for that domain. Konductor SHALL derive the canonical schema name and SHALL use the AWS Schemas version-producing operation so existing versions remain immutable. The appended `content` SHALL parse as a valid JSON Schema draft-4 document, and SHALL be rejected with `400 Bad Request` otherwise.

Before writing, Konductor SHALL read the current latest version in the stage-selected registry as the comparison predecessor. An accepted candidate SHALL preserve every payload accepted by that predecessor under the application's supported Draft-4 validation semantics. Acceptance SHALL require a sound proof within the supported comparison rules and bounded comparison work; unsupported interactions, precision-sensitive comparisons without a sound rule, and exhausted proof budgets SHALL fail closed with `400 Bad Request` and no registry write. Compatibility SHALL be judged against the predecessor read for this append, never historical versions chosen by the caller. This pairwise check SHALL NOT be represented as serialization of concurrent appends or as protection against a competing write between predecessor lookup and registry update.

New optional properties SHALL NOT be accepted solely because they are optional: their constraints SHALL preserve values the predecessor allowed for those property names, including applicable additional-property and pattern-property constraints. In the supported reference-free domain, adding an optional property to an object that previously prohibited that name SHALL be accepted when the other constraints remain compatible. Adding a new constraint to a previously unrestricted property name SHALL be rejected unless acceptance of all previously allowed values can be established.

The gate SHALL preserve supported proofs for identical documents, dropped `required` entries, type/enum widening, and loosened or removed constraints, subject to the proof budget and applicable interaction checks. Changed schema-bearing references, reference-dependent changes, and other unsupported semantics SHALL NOT be accepted merely because local keywords appear unchanged or loosened. Numeric containment SHALL NOT be inferred from lossy conversion or rounded arithmetic. The gate SHALL continue to conservatively reject removed declared properties and other unproven changes; it SHALL NOT imply support for complete semantic implication checking.

Rejections SHALL identify relevant paths and reasons using the existing validation-error response envelope, with at most ten reported paths and an indication when more reasons exist. Explanations SHALL distinguish local constraint restrictions from unsupported proof cases and SHALL NOT describe a change as demonstrably incompatible solely because proof was unavailable. The gate SHALL remain absolute: no request flag, role, or binding SHALL bypass it. Intentional breaking evolution SHALL be expressed as a new schema under a new event name, never as a new version. Schema creation (`POST /schemas`) SHALL NOT be compatibility-gated because version 1 has no predecessor. Publisher APIs SHALL NOT expose schema deletion, retirement, version deletion, or historical-version replacement.

#### Scenario: Authorized publisher appends a version

- **WHEN** a caller with a `publisher` binding for domain `orders` appends a supported provably compatible schema document for event `OrderNotification` on the `dev` stage, where `dev` is the authoring environment
- **THEN** Konductor SHALL create the next version of `{namespaceIdentity}@OrderNotification` in `{org}-dev-orders-schema-registry`
- **AND THEN** all earlier versions SHALL remain unchanged
- **AND THEN** the API SHALL return `201 Created` with the schema name and new version

#### Scenario: Additive append is accepted

- **WHEN** a publisher appends a supported reference-free document that adds an optional property previously prohibited by `additionalProperties: false`, drops a `required` entry, or widens a type relative to the comparison predecessor
- **AND WHEN** the remaining constraints are provably compatible and the comparison stays within its proof budget
- **THEN** the API SHALL return `201 Created` with the new version

#### Scenario: Tightening append is rejected

- **WHEN** a publisher appends a document that newly requires a property the comparison predecessor treats as optional
- **THEN** the API SHALL return `400 Bad Request` identifying the offending path and local restriction
- **AND THEN** no new version SHALL be created

#### Scenario: Constraint tightening is rejected

- **WHEN** a publisher appends a document that raises a `minimum`, lowers a `maximum`, shrinks an `enum`, or flips `additionalProperties` to `false` relative to the comparison predecessor
- **THEN** the API SHALL return `400 Bad Request` identifying the relevant path and reason
- **AND THEN** no new version SHALL be created

#### Scenario: Removed property is rejected

- **WHEN** a publisher appends a document that omits a property declared by the comparison predecessor
- **THEN** the API SHALL return `400 Bad Request` identifying the relevant path and the unsupported property-removal proof
- **AND THEN** no new version SHALL be created

#### Scenario: Compatibility is judged against the latest version only

- **WHEN** the latest version read for an append is v4 and a publisher submits a document compatible with v3 but tightening relative to v4
- **THEN** the API SHALL return `400 Bad Request`
- **AND THEN** compatibility with earlier historical versions SHALL NOT rescue the append

#### Scenario: The gate admits no override

- **WHEN** any caller, including one holding a `platform-admin` role, appends a tightening document or a document whose compatibility cannot be established
- **THEN** the API SHALL return `400 Bad Request`
- **AND THEN** no request flag SHALL exist that forces acceptance

#### Scenario: Malformed version content rejected

- **WHEN** a publisher appends content that does not parse as a valid JSON Schema draft-4 document
- **THEN** the API SHALL return `400 Bad Request` identifying the failure
- **AND THEN** no new version SHALL be created

#### Scenario: Version target does not exist

- **WHEN** a caller requests an otherwise valid and authorized append for an event whose canonical schema does not exist in the stage-selected registry
- **THEN** the API SHALL return `404 Not Found`
- **AND THEN** no schema SHALL be created implicitly

#### Scenario: Publisher mutation surface is append-only

- **WHEN** a publisher inspects or invokes the schema-management API
- **THEN** no publisher operation SHALL exist to delete or retire a schema, delete a version, or replace a historical version

#### Scenario: Optional property can still break existing payloads

- **WHEN** the predecessor is `{"properties":{"sku":{}}}` and the candidate is `{"properties":{"sku":{},"note":{"type":"string"}}}`
- **THEN** the append SHALL return `400 Bad Request` identifying `/note`
- **AND THEN** the gate SHALL NOT overlook that `{"sku":"A1","note":42}` was previously accepted
- **AND THEN** no new version or schema-version publication event SHALL be produced

#### Scenario: Additional-property subschema governs a new named property

- **WHEN** an object previously allowed unnamed properties according to `{"type":"number"}` and a candidate introduces a named optional property restricted to `{"type":"string"}`
- **THEN** the append SHALL be rejected with `400 Bad Request` identifying that property
- **AND THEN** the optional status SHALL NOT bypass the predecessor's allowed value set

#### Scenario: Unchanged reference text does not establish compatibility

- **WHEN** the predecessor is `{"properties":{"a":{"type":"integer"},"b":{"not":{"$ref":"#/properties/a"}}}}` and the candidate changes only `a` to `{}`
- **THEN** the append SHALL be rejected with `400 Bad Request` explaining the reference-dependent proof limitation
- **AND THEN** the gate SHALL NOT accept the local loosening while overlooking that `{"a":1,"b":"text"}` becomes invalid

#### Scenario: Identical referenced document remains appendable

- **WHEN** a publisher submits an identical valid document containing local references and the comparison is within its proof budget
- **THEN** reference presence alone SHALL NOT cause a rejection
- **AND THEN** the authorized append SHALL retain its normal success behavior

#### Scenario: Precision loss cannot authorize an append

- **WHEN** two distinct numeric constraints appear equal only after a lossy numeric conversion and inclusion cannot be established under the payload validator's semantics
- **THEN** the append SHALL return `400 Bad Request` identifying the numeric proof limitation
- **AND THEN** the converted equality SHALL NOT authorize a registry write

#### Scenario: Unsupported proof is reported accurately

- **WHEN** a candidate changes a pattern or other unsupported construct without an available compatibility proof
- **THEN** the API SHALL return `400 Bad Request` naming the relevant path and proof limitation
- **AND THEN** the message SHALL NOT assert that a breaking payload has been demonstrated

#### Scenario: Comparison budget exhaustion fails closed

- **WHEN** establishing compatibility exceeds the documented comparison work budget
- **THEN** the API SHALL return `400 Bad Request` explaining that compatibility could not be established within the limit
- **AND THEN** no partial proof SHALL authorize a registry write or schema-version publication event
### Requirement: Publisher schema operations do not grant registry administration

Publisher authorization SHALL permit only schema creation and version appending in the authoring environment's registries of bound domains, and promotion of those domains' schemas into later environments' registries. It SHALL NOT permit publishers to create, update, or delete registries or to administer registry policies. Registry lifecycle and policy administration SHALL remain platform-admin operations.

#### Scenario: Publisher attempts registry administration

- **WHEN** a caller has a `publisher` binding for a domain
- **THEN** that binding SHALL NOT authorize any registry lifecycle or registry policy operation

#### Scenario: One binding covers multiple environments

- **WHEN** a caller's team group has a `publisher` binding for domain `orders`
- **THEN** the caller MAY author `orders` schemas on the authoring environment's stage and promote them into every later environment
- **AND THEN** the binding SHALL NOT need to name an environment

#### Scenario: AWS account binding uses identical schema semantics

- **WHEN** an IAM caller's normalized AWS account has a `publisher` binding for domain `orders`
- **THEN** the IAM surface SHALL authorize the same schema operations and enforce the same domain constraints as the OAuth surface
### Requirement: Schema discovery is read-only

The schema-management capability SHALL provide schema creation, append-only version registration, and promotion only through the publisher-authorized operations defined by `add-publisher-apis` and `promote-schema-versions`. It SHALL NOT provide publisher operations for schema replacement, retirement, version deletion, schema deletion, registry lifecycle, or registry policy administration. Discovery SHALL be filtered by the caller-visible consumer domain's current entitlements; the interim baseline in which any authenticated platform caller could discover any active domain's schemas SHALL NOT remain available.

#### Scenario: Only append-only write operations exposed

- **WHEN** a publisher uses the Konductor schema API
- **THEN** the only schema write operations exposed SHALL be schema creation, appending a new schema version, and promotion
- **AND THEN** promotion SHALL only append versions copied from the source environment

#### Scenario: Global visibility baseline is retired

- **WHEN** any caller requests schema discovery without an entitlement-covered consumer domain or a platform role
- **THEN** the response SHALL contain no schemas
- **AND THEN** the pre-existing behavior of serving all active domains' schemas to any authenticated caller SHALL NOT remain available
### Requirement: Schema version publication events

The control plane SHALL emit a `SchemaVersionPublished` event on the control-plane bus (source `konductor.schema-management`) after every successful schema create or append in the authoring environment, and exactly once per promotion after its last registry write, carrying `producerDomain`, `schemaName`, `environment` (the environment whose registry was written), `version` (for a promotion, the last version it wrote or, when it wrote none, the target's latest), and `publishedAt`. This event is the platform's promotion signal: it is how consumers' subscriptions learn that a schema has arrived in a new environment. Replay steps SHALL NOT emit their own events. For create and append, emission follows the committed registry write; a failed emission SHALL be logged and SHALL NOT fail the API response, since the registry write is the committed state and appends are not safely retryable (a retry would mint a further version). A promotion SHALL retry its emission durably, which is safe because promotion-following extension is idempotent.

#### Scenario: Create emits version 1 for the target environment

- **WHEN** a publisher successfully registers `OrderNotification` on the authoring environment `dev`
- **THEN** a `SchemaVersionPublished` event SHALL be emitted with `environment: dev` and `version: 1`

#### Scenario: Append emits the new version

- **WHEN** a publisher successfully appends version 5 of a schema in the authoring environment `dev`
- **THEN** a `SchemaVersionPublished` event SHALL be emitted with `environment: dev` and `version: 5`

#### Scenario: Promotion emits once with its final version

- **WHEN** a promotion into staging writes v2 and v3
- **THEN** exactly one `SchemaVersionPublished` event SHALL be emitted for that promotion, with `environment: staging` and `version: 3`

#### Scenario: Partial promotion signals the last written version

- **WHEN** a promotion into staging writes v2 and fails before writing v3
- **THEN** one `SchemaVersionPublished` event SHALL be emitted with `environment: staging` and `version: 2`

#### Scenario: Emission failure does not fail the write

- **WHEN** the registry write of a create or append has succeeded but emitting the event fails
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

### Requirement: Schema authoring is restricted to the authoring environment

Schema creation (`POST /schemas`) and version append (`POST /schemas/versions`) SHALL be accepted only on the stage of the authoring environment, the first entry of the platform environment list. On any other stage both surfaces SHALL return `400 Bad Request` naming the authoring environment and stating that versions reach later environments only through promotion, before any registry access. No role, binding, or request flag SHALL bypass this rule.

#### Scenario: Append on a later stage is rejected

- **WHEN** a caller with a `publisher` binding for `orders` appends a version of `OrderNotification` on the `staging` stage of a `["dev", "staging", "prod"]` installation
- **THEN** the API SHALL return `400 Bad Request` naming `dev` as the authoring environment and promotion as the path into `staging`
- **AND THEN** no registry read or write SHALL occur

#### Scenario: Create on a later stage is rejected

- **WHEN** a publisher requests schema creation on the `prod` stage
- **THEN** the API SHALL return `400 Bad Request`
- **AND THEN** no schema SHALL be created in any registry

#### Scenario: Platform administrators follow the same rule

- **WHEN** a `platform-admin` caller appends a version on a stage other than the authoring environment's
- **THEN** the API SHALL return `400 Bad Request`

#### Scenario: Both surfaces enforce the rule

- **WHEN** an IAM caller with an account `publisher` binding creates a schema on a stage other than the authoring environment's
- **THEN** the IAM surface SHALL reject it exactly as the OAuth surface does

### Requirement: Schema promotion request

Both API surfaces SHALL provide `POST /schemas/promotions` accepting `domain` and `eventName`. The stage SHALL select the target environment and the source SHALL be the environment immediately before it in the platform environment list. The grant engine SHALL allow `schema:promote` for the domain, which the `publisher` role grants. An accepted request SHALL return `202 Accepted` with a promotion identifier and SHALL complete asynchronously.

The promotion workflow SHALL verify the same adjacency invariant before its first registry read: when the input's source is not the target's immediate predecessor (the pair the API would have derived), the promotion SHALL fail with reason `SKIPPED_ENVIRONMENT`, writing no version and changing no rule. This gates executions started out-of-band with a planted lease.

#### Scenario: Out-of-band execution with a skipped environment

- **WHEN** a promotion execution carries a source that is not the target's immediate predecessor (for example dev straight into prod)
- **THEN** the promotion SHALL fail with reason `SKIPPED_ENVIRONMENT` before any registry read
- **AND THEN** no version SHALL be written and no rule SHALL change

#### Scenario: Publisher promotes into the next environment

- **WHEN** a caller with a `publisher` binding for `orders` requests promotion of `OrderNotification` on the `staging` stage
- **THEN** the API SHALL return `202 Accepted` with a promotion identifier
- **AND THEN** the promotion SHALL use `dev` as its source and `staging` as its target

#### Scenario: Promotion into the authoring environment is rejected

- **WHEN** a publisher requests promotion on the authoring environment's stage
- **THEN** the API SHALL return `400 Bad Request` stating that the authoring environment has no source

#### Scenario: Caller lacks promote authorization

- **WHEN** a caller without a `publisher` binding for `orders` requests promotion of an `orders` schema
- **THEN** the API SHALL return `403 Forbidden`
- **AND THEN** no promotion SHALL be recorded

#### Scenario: Schema absent from the source

- **WHEN** a publisher requests promotion into `prod` of a schema that does not exist in the `staging` registry
- **THEN** the API SHALL return `404 Not Found` identifying the schema and source environment

#### Scenario: Inactive or missing domain

- **WHEN** a publisher requests promotion for a domain that does not exist or is not `ACTIVE`
- **THEN** the API SHALL return `404 Not Found`
- **AND THEN** no promotion SHALL be recorded

### Requirement: Promotion replays missing versions in order

A promotion SHALL write each source version newer than the target's latest into the target registry in ascending order, using the version's recorded content (see Schema version content is durably recorded), so version N holds canonically equal content in source and target. Replayed versions SHALL NOT be re-evaluated by the compatibility gate; their canonical match with the already-gated source versions is the acceptance condition.

#### Scenario: Staging catches up to dev

- **WHEN** dev holds v1, v2, v3 and staging holds a v1 canonically equal to dev v1, and `OrderNotification` is promoted into staging
- **THEN** staging SHALL receive v2 and then v3, each canonically equal to the dev version of the same number
- **AND THEN** staging's latest version SHALL be 3

#### Scenario: First arrival creates version 1

- **WHEN** a schema exists in dev but not in staging, and it is promoted into staging
- **THEN** staging SHALL receive every dev version starting at v1, in order

#### Scenario: Replayed versions are not re-gated

- **WHEN** a dev version accepted under an earlier compatibility gate would be rejected by the current gate, and it is replayed into staging
- **THEN** the replay SHALL write it because its content matches the source exactly

#### Scenario: Promotion with nothing to replay still converges

- **WHEN** a schema is promoted into an environment whose latest version already equals the source's latest
- **THEN** no version SHALL be written
- **AND THEN** the promotion SHALL still re-pin subscriptions and emit its completion signal as for any other promotion

### Requirement: Schema version content is durably recorded

Konductor SHALL record the canonical content, a content hash, and the writer of every version created or appended in the authoring environment in a durable version-content store; registries normalize content and carry no provenance, so this store is the basis of replay and divergence detection. Replay and prefix verification SHALL resolve source-version content from this store first, falling back to the source registry's immutable per-version content for versions that predate recording; a source version found in neither SHALL fail the promotion.

#### Scenario: Append records the new version's content

- **WHEN** a publisher successfully appends v4 in the authoring environment
- **THEN** the version-content store SHALL hold v4's canonical content and hash for the authoring environment
- **AND THEN** a later promotion SHALL replay v4 from that record

#### Scenario: A schema that predates content recording promotes from the registry

- **GIVEN** a schema whose source versions were created before content recording existed, so the store holds no records for them
- **WHEN** the schema is promoted
- **THEN** replay SHALL read each missing source version's content from the source registry, whose per-version content is immutable
- **AND THEN** the promotion SHALL proceed as for recorded versions, writing promotion-owned records in the target

#### Scenario: Ambiguous append retry completes the record

- **WHEN** an append's registry write succeeded but its content record was not written
- **AND WHEN** the publisher retries the append with canonically equal content
- **THEN** no new version SHALL be minted
- **AND THEN** the missing record SHALL be completed for the existing version
- **AND THEN** the response SHALL identify the existing version as a duplicate append

### Requirement: Diverged targets fail closed

Before writing, a promotion SHALL verify from the version-content store that every target version 1..k (k = the target's latest) was written by an earlier promotion from content canonically equal to the same-numbered source version. Any mismatch, missing write record, or target ahead of its source SHALL fail with reason `DIVERGED`, writing no version and changing no rule.

#### Scenario: Content mismatch is detected

- **WHEN** staging v2 differs from dev v2 and a promotion into staging is requested
- **THEN** the promotion SHALL fail with reason `DIVERGED` identifying the first differing version
- **AND THEN** no version SHALL be written to staging and no rule SHALL change

#### Scenario: Target ahead of its source

- **WHEN** prod holds more versions than staging and a promotion into prod is requested
- **THEN** the promotion SHALL fail with reason `DIVERGED`

#### Scenario: Missing record for the readable latest version is repaired

- **WHEN** the target's latest version lacks a recorded write but its registry content is canonically equal to the source's record for that version
- **THEN** the promotion SHALL complete the record and proceed
- **AND THEN** a missing record for any older version SHALL fail with reason `DIVERGED`, because only the latest version is registry-readable

### Requirement: Interrupted promotions resume without duplicating versions

A promotion's registry writes SHALL be safe to retry: a retried or repeated replay step SHALL NOT create a second version for the same source version. A promotion requested after an earlier one failed partway SHALL continue from the target's latest version, using the same prefix verification.

#### Scenario: Promotion resumes after a partial failure

- **WHEN** a promotion into staging wrote v2 and then failed before writing v3
- **AND WHEN** a publisher requests promotion into staging again
- **THEN** the new promotion SHALL verify v1 and v2 and write only v3

#### Scenario: Retried step does not mint a duplicate

- **WHEN** a replay write is retried after an ambiguous failure
- **THEN** the target SHALL hold exactly one version for that source version

### Requirement: One active promotion per schema and target

Konductor SHALL allow at most one unfinished promotion for each schema and target environment. A request made while one is unfinished SHALL return `409 Conflict` identifying the active promotion, and SHALL NOT be queued.

#### Scenario: Concurrent promotion is rejected

- **WHEN** a promotion of `OrderNotification` into staging is in progress and another is requested for the same schema and target
- **THEN** the API SHALL return `409 Conflict` with the active promotion's identifier

#### Scenario: Different targets proceed independently

- **WHEN** a promotion into staging is in progress and a promotion of the same schema into prod is requested
- **THEN** the prod request SHALL be accepted, using staging's current versions as its source

### Requirement: Promotion progress is observable

Both surfaces SHALL provide `GET /schemas/promotions?domain={domain}&eventName={eventName}` on the target stage, returning the most recent promotion for that schema and target. It SHALL report the identifier, source and target, starting (`fromVersion`), requested, and last written versions, phase (`VALIDATING`, `REPLAYING`, `REPINNING`, `COMPLETED`, `FAILED`, `CANCELLED`), any failure reason, the actor who requested it, and timestamps. Callers SHALL hold a `publisher` binding on the domain or a platform role.

#### Scenario: Publisher reads an in-flight promotion

- **WHEN** a publisher reads promotion status for `OrderNotification` on the staging stage while re-pinning is under way
- **THEN** the response SHALL report phase `REPINNING`, the last written version, and the requesting actor

#### Scenario: Partial failure is reported

- **WHEN** a promotion failed after writing v2 of a requested v3
- **THEN** the status SHALL report phase `FAILED`, reason `REPLAY_FAILED`, requested version 3, and last written version 2

#### Scenario: No promotion yet

- **WHEN** status is requested for a schema never promoted into that environment
- **THEN** the API SHALL return `404 Not Found`

#### Scenario: Unauthorized reader

- **WHEN** a caller without a `publisher` binding or platform role requests promotion status
- **THEN** the API SHALL return `403 Forbidden`

### Requirement: Promotion lifecycle events

Konductor SHALL emit, on the control-plane bus with source `konductor.schema-management`: `SchemaPromotionStarted` after the prefix verification passes, `SchemaPromotionCompleted` after narrowing, and `SchemaPromotionFailed` with reason `DIVERGED` or `REPLAY_FAILED`. Events SHALL carry the promotion identifier, schema identity, source and target, and versions, and SHALL derive from committed promotion state.

#### Scenario: Successful promotion emits started then completed

- **WHEN** a promotion from v1 to v3 into staging succeeds
- **THEN** `SchemaPromotionStarted` and then `SchemaPromotionCompleted` SHALL be emitted with the same promotion identifier, from-version 1, and to-version 3

#### Scenario: Divergence emits only a failure

- **WHEN** a promotion fails its prefix verification
- **THEN** `SchemaPromotionFailed` with reason `DIVERGED` SHALL be emitted
- **AND THEN** no `SchemaPromotionStarted` SHALL be emitted

#### Scenario: Completion reports re-pin results

- **WHEN** a promotion completes and one of four target subscriptions failed to re-pin
- **THEN** `SchemaPromotionCompleted` SHALL report three re-pinned subscriptions and one re-pin failure

### Requirement: Per-environment schema content is readable

Both API surfaces SHALL provide `GET /schemas/{schemaName}/content` accepting `domain` and `environment` query parameters plus an optional `version` parameter, returning the named version's content from that environment's registry. When `version` is omitted, the environment's latest version SHALL be returned. Content SHALL be read from the environment's registry — whose per-version content is immutable — never from the version-content store, so schemas that predate content recording remain readable. Callers SHALL hold a `publisher` binding on the producer domain or a platform role, the same rule as the schema detail route, and there SHALL be no consumer-context path. A schema absent from the named environment, or a version that does not exist, SHALL return `404 Not Found`; an unauthorized caller SHALL receive `403 Forbidden`. The response SHALL identify the environment and the version returned.

#### Scenario: Publisher reads an environment's latest content

- **WHEN** a publisher bound to `sales` requests `GET /schemas/acme-sales@OrderPlaced/content?domain=sales&environment=staging` without a version
- **THEN** the response SHALL contain staging's latest version content and identify the version returned

#### Scenario: A specific version is read

- **WHEN** a publisher requests the content with `version=1` while the environment's latest is v3
- **THEN** the response SHALL contain v1's content, read from the registry's immutable per-version record

#### Scenario: Schema absent from the environment

- **WHEN** the schema exists in dev but has never been promoted to prod, and its content is requested for prod
- **THEN** the API SHALL return `404 Not Found`

#### Scenario: Unknown version

- **WHEN** the content is requested with a version the environment's registry does not hold
- **THEN** the API SHALL return `404 Not Found`

#### Scenario: Caller lacks producer access

- **WHEN** a caller with no `publisher` binding on the producer domain and no platform role requests content
- **THEN** the API SHALL return `403 Forbidden`

#### Scenario: Both surfaces enforce the rule

- **WHEN** the same content request is issued against the OAuth and IAM stages
- **THEN** both SHALL apply the identical authorization and response semantics
