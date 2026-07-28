## ADDED Requirements

### Requirement: Publisher schema creation API

The separate OAuth and IAM control-plane surfaces SHALL each provide `POST /domains/{domain}/environments/{environment}/schemas` for creating an event schema in the selected domain registry where interactive and automation compatibility is required. Both surfaces SHALL normalize their authenticated principal and require the DynamoDB-backed grant engine to allow the publisher action for that domain through an applicable Cognito team-group or AWS-account binding. The request SHALL identify the event by event name and provide the schema document; Nexus SHALL derive the canonical schema name `{source}@{eventName}` and registry from the active domain and selected environment. Both route contracts SHALL reject a duplicate schema rather than replacing it.

#### Scenario: Authorized publisher creates a schema

- **WHEN** a caller with a `publisher` binding for domain `orders` creates event `OrderNotification` in `dev`
- **THEN** Nexus SHALL create schema `prezzee-orders@OrderNotification` in registry `prezzee-dev-orders-schema-registry`
- **AND THEN** the schema SHALL be created as version 1

#### Scenario: Caller lacks a binding for the target domain

- **WHEN** an authenticated caller without a `publisher` binding for domain `orders` requests schema creation for `orders`
- **THEN** the API SHALL return `403 Forbidden`
- **AND THEN** no schema SHALL be created

#### Scenario: Existing schema is not replaced

- **WHEN** a publisher requests creation of an event whose canonical schema name already exists in the selected registry
- **THEN** the API SHALL return `409 Conflict`
- **AND THEN** the existing schema and all of its versions SHALL remain unchanged

#### Scenario: Inactive or missing domain

- **WHEN** a publisher requests schema creation for a domain that does not exist or is not `ACTIVE`
- **THEN** the API SHALL return `404 Not Found`
- **AND THEN** no registry write SHALL occur

### Requirement: Append-only schema version API

The separate OAuth and IAM control-plane surfaces SHALL each provide `POST /domains/{domain}/environments/{environment}/schemas/{eventName}/versions` for appending a schema version in the selected domain registry where interactive and automation compatibility is required. Both surfaces SHALL normalize their authenticated principal and require the DynamoDB-backed grant engine to allow the publisher action for that domain. Nexus SHALL derive the canonical schema name and SHALL use the Schemas API version operation so existing versions remain immutable. Publisher APIs SHALL NOT expose schema deletion, retirement, version deletion, or historical-version replacement.

#### Scenario: Authorized publisher appends a version

- **WHEN** a caller with a `publisher` binding for domain `orders` appends a schema document for event `OrderNotification` in `staging`
- **THEN** Nexus SHALL create the next version of `prezzee-orders@OrderNotification` in `prezzee-staging-orders-schema-registry`
- **AND THEN** all earlier versions SHALL remain unchanged

#### Scenario: Version target does not exist

- **WHEN** a publisher attempts to append a version for an event whose canonical schema does not exist in the selected registry
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
- **THEN** the caller MAY use publisher schema operations for any supported environment of `orders`
- **AND THEN** the caller SHALL select the environment explicitly on each request

#### Scenario: AWS account binding uses identical schema semantics

- **WHEN** an IAM caller's normalized AWS account has a `publisher` binding for domain `orders`
- **THEN** the IAM surface SHALL authorize the same schema operations and enforce the same domain and environment constraints as the OAuth surface

## MODIFIED Requirements

### Requirement: Discovery is read-only in this capability version

The schema-management capability SHALL provide schema creation and append-only version registration only through the publisher-authorized operations defined in this capability. It SHALL NOT provide publisher operations for schema replacement, retirement, version deletion, schema deletion, registry lifecycle, or registry policy administration. This change SHALL NOT alter the existing behavior in which any authenticated platform caller may discover any active domain's schemas. That discovery behavior is an interim baseline subject to the later `add-subscriber-entitlements` change and SHALL NOT be interpreted as the final combined visibility policy.

#### Scenario: Only append-only write operations exposed

- **WHEN** a publisher uses the Nexus schema API
- **THEN** the only schema write operations exposed SHALL be schema creation and appending a new schema version

#### Scenario: Discovery behavior is unchanged by publisher APIs

- **WHEN** any authenticated caller requests schema discovery for an active domain
- **THEN** this change SHALL preserve the pre-existing discovery response regardless of the caller's relationship to the domain
- **AND THEN** this interim requirement SHALL NOT constrain the final visibility behavior specified by the later `add-subscriber-entitlements` change
