# Delta: schema-management

## MODIFIED Requirements

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
