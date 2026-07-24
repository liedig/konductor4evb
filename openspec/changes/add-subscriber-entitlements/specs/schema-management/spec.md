## MODIFIED Requirements

### Requirement: Schema discovery

The control plane SHALL replace global schema discovery with a dedicated, read-only schema catalog for each consumer domain while preserving publisher-authorized schema creation and append-only version APIs. On both OAuth and IAM API surfaces, `GET /domains/{consumerDomain}/catalog/schemas` SHALL list the union of schemas covered by that consumer domain's current producer-domain and exact producer-schema entitlements, merged across environments and keyed by schema name. Each list item SHALL contain schema metadata and the latest version in each selected environment, but SHALL NOT include full schema content. `GET /domains/{consumerDomain}/catalog/schemas/{schemaName}` SHALL return metadata, per-environment latest versions, and the full latest schema content. Catalog data SHALL be expanded from the producer domains' per-environment schema registries without introducing a duplicate schema table.

Both catalog operations SHALL require subscriber access to the consumer domain through a Cognito team-group binding on the OAuth surface or an AWS-account binding on the IAM surface, using the same authorization decision. A detail request SHALL additionally require a current entitlement covering the requested producer schema. Catalog lists SHALL support producer-domain and environment filters and pagination. Producer-domain grants SHALL include all current and future schemas from that producer, exact-schema grants SHALL include only the named schema, overlapping grants SHALL yield one catalog entry, and logical grants SHALL apply across all environments. Unauthorized, revoked, inactive-domain, or missing-schema detail requests SHALL return `404 Not Found` without revealing which condition failed.

#### Scenario: List entitlement-expanded catalog

- **WHEN** an authorized subscriber requests `GET /domains/checkout/catalog/schemas`
- **THEN** the response SHALL list the union of schemas covered by `checkout`'s current producer-domain and exact-schema entitlements
- **AND THEN** each entry SHALL include metadata and latest version per environment without full schema content

#### Scenario: Producer-domain grant reveals a future schema

- **WHEN** `checkout` has a producer-domain grant for `orders` and a new schema is added to an `orders` registry
- **THEN** a subsequent catalog list SHALL include the new schema without changing the grant

#### Scenario: Catalog list supports filters and pagination

- **WHEN** an authorized subscriber lists a catalog with producer-domain and environment filters and a page token
- **THEN** the response SHALL contain only matching schemas for the selected environment scope and SHALL return the next page token when more results remain

#### Scenario: Detail returns full schema content

- **WHEN** an authorized subscriber requests detail for a currently entitled schema
- **THEN** the response SHALL include its metadata, latest version in each environment, and full latest schema content

#### Scenario: Revoked schema detail is concealed

- **WHEN** a caller requests schema detail after the last entitlement covering that schema is revoked
- **THEN** the API SHALL return `404 Not Found`
- **AND THEN** the response SHALL NOT disclose whether the schema exists

### Requirement: Discovery is read-only in this capability version

The schema-management capability SHALL provide publisher-authorized schema creation and append-only version registration through `POST /domains/{domain}/environments/{environment}/schemas` and `POST /domains/{domain}/environments/{environment}/schemas/{eventName}/versions`, as defined by the `add-publisher-apis` prerequisite. It SHALL NOT provide publisher operations for schema replacement, retirement, version deletion, schema deletion, registry lifecycle, or registry policy administration. Global discovery SHALL be replaced by the consumer-domain catalog, whose visibility is limited to callers with subscriber access to the consumer domain and schemas covered by that domain's current entitlements.

#### Scenario: Publisher write APIs remain available

- **WHEN** an authorized publisher creates a schema or appends its next immutable version through the publisher API
- **THEN** Nexus SHALL process that operation according to the publisher schema-management contract

#### Scenario: Destructive publisher operations remain unavailable

- **WHEN** a publisher inspects or invokes the schema-management API
- **THEN** no publisher operation SHALL exist to replace, retire, or delete a schema or historical version or to administer a registry

#### Scenario: Authenticated caller without subscriber access is denied

- **WHEN** an OAuth or IAM caller without subscriber access to the consumer domain requests its catalog
- **THEN** Nexus SHALL deny access
