## MODIFIED Requirements

### Requirement: Schema discovery

The control plane SHALL provide read-only schema discovery as an entitlement-filtered consumer catalog. `GET /schemas?consumerDomain={consumerDomain}` SHALL list exactly the schemas covered by the consumer domain's current entitlements, expanded across environments from the per-environment registries (source of truth), deduplicating overlapping domain-wide and exact-schema coverage, and reporting the latest version per environment. Producer-domain and environment filters MAY narrow the expansion, and pagination SHALL use an opaque continuation token carrying cursors only, never authorization state. `GET /schemas/{schemaName}?consumerDomain={consumerDomain}&domain={producerDomain}` SHALL return schema detail including per-environment latest versions and the latest content under the same coverage check. Both routes SHALL require the caller to hold a `subscriber` binding on the queried consumer domain. `platform-admin` and `platform-auditor` callers MAY omit `consumerDomain` and receive the platform-wide catalog unfiltered by entitlements. No separate schema or catalog table SHALL be introduced.

#### Scenario: Catalog expands current entitlements

- **WHEN** a caller with a `subscriber` binding on `checkout` requests `GET /schemas?consumerDomain=checkout`
- **THEN** the response SHALL list exactly the schemas covered by `checkout`'s current entitlements, keyed by schema name, with the latest version per environment

#### Scenario: Version skew visible before subscribing

- **WHEN** a covered schema's latest version differs across environments (e.g., dev: 5, staging: 3, prod: 3)
- **THEN** the catalog response SHALL report each environment's latest version distinctly, so consumers see skew before signing the contract

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

- **WHEN** a `platform-admin` or `platform-auditor` caller requests `GET /schemas` without `consumerDomain`
- **THEN** the response SHALL span all discoverable schemas unfiltered by entitlements
- **AND THEN** `platform-auditor` SHALL NOT be able to perform any mutation

### Requirement: Discovery is read-only in this capability version

The schema-management capability SHALL provide schema creation and append-only version registration only through the publisher-authorized operations defined by `add-publisher-apis`. It SHALL NOT provide publisher operations for schema replacement, retirement, version deletion, schema deletion, registry lifecycle, or registry policy administration. This change establishes the final discovery visibility policy: discovery SHALL be filtered by the caller-visible consumer domain's current entitlements, replacing the interim baseline in which any authenticated platform caller could discover any active domain's schemas.

#### Scenario: Only append-only write operations exposed

- **WHEN** a publisher uses the Nexus schema API
- **THEN** the only schema write operations exposed SHALL be schema creation and appending a new schema version

#### Scenario: Global visibility baseline is retired

- **WHEN** any caller requests schema discovery without an entitlement-covered consumer domain or a platform role
- **THEN** the response SHALL contain no schemas
- **AND THEN** the pre-existing behavior of serving all active domains' schemas to any authenticated caller SHALL NOT remain available
