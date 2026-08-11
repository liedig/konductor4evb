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

The separate OAuth and IAM control-plane surfaces SHALL each provide `POST /schemas` as a flat verb endpoint for creating an event schema. The request SHALL accept `domain`, `eventName`, `schemaType`, and `content` in the request body. The target environment SHALL be inferred from the API Gateway stage (`$context.stage`). Both surfaces SHALL normalize their authenticated principal and require the DynamoDB-backed grant engine to allow the publisher action for that domain. Nexus SHALL derive the canonical schema name `{namespaceIdentity}@{eventName}` and the stage-selected environment's registry. Both route contracts SHALL reject a duplicate schema rather than replacing it.

#### Scenario: Authorized publisher creates a schema

- **WHEN** a caller with a `publisher` binding for domain `orders` creates event `OrderNotification` on the `dev` stage
- **THEN** Nexus SHALL create schema `{namespaceIdentity}@OrderNotification` in registry `{org}-dev-orders-schema-registry`
- **AND THEN** the schema SHALL be created as version 1
- **AND THEN** the API SHALL return `201 Created` with the schema name and version

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

The separate OAuth and IAM control-plane surfaces SHALL each provide `POST /schemas/versions` as a flat verb endpoint for appending a schema version. The request SHALL accept `domain`, `eventName`, and `content` in the request body. The target environment SHALL be inferred from the API Gateway stage. Both surfaces SHALL normalize their authenticated principal and require the grant engine to allow the publisher action for that domain. Nexus SHALL derive the canonical schema name and SHALL use the AWS Schemas version-producing operation so existing versions remain immutable. Publisher APIs SHALL NOT expose schema deletion, retirement, version deletion, or historical-version replacement.

#### Scenario: Authorized publisher appends a version

- **WHEN** a caller with a `publisher` binding for domain `orders` appends a schema document for event `OrderNotification` on the `staging` stage
- **THEN** Nexus SHALL create the next version of `{namespaceIdentity}@OrderNotification` in `{org}-staging-orders-schema-registry`
- **AND THEN** all earlier versions SHALL remain unchanged
- **AND THEN** the API SHALL return `201 Created` with the schema name and new version

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

- **WHEN** a publisher uses the Nexus schema API
- **THEN** the only schema write operations exposed SHALL be schema creation and appending a new schema version

#### Scenario: Global visibility baseline is retired

- **WHEN** any caller requests schema discovery without an entitlement-covered consumer domain or a platform role
- **THEN** the response SHALL contain no schemas
- **AND THEN** the pre-existing behavior of serving all active domains' schemas to any authenticated caller SHALL NOT remain available
