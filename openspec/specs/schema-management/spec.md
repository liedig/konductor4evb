# Capability: Schema Management

## Purpose

How event schemas are discovered and managed: read-only discovery across each domain's per-environment registries, plus publisher-authorized schema creation and append-only version registration in platform-managed registries, with the target environment inferred from the API stage.

## Requirements

### Requirement: Schema discovery

The control plane SHALL provide read-only schema discovery so consumers can find event contracts to subscribe to. `GET /schemas?domain={producerDomain}` SHALL list the schemas in the producer's registries, merged across environments and keyed by schema name, reporting the latest version per environment. `GET /schemas/{schemaName}?domain={producerDomain}` SHALL return schema detail including per-environment latest versions and the schema content for the latest version. Discovery SHALL read from the per-environment schema registries directly (source of truth for contracts) — no separate schema table SHALL be introduced.

#### Scenario: List a domain's schemas

- **WHEN** a caller requests `GET /schemas?domain=orders`
- **THEN** the response SHALL list each schema in the `orders` registries (e.g., `prezzee-orders@OrderNotification`) with the latest version per environment

#### Scenario: Version skew visible before subscribing

- **WHEN** a schema's latest version differs across environments (e.g., dev: 5, staging: 3, prod: 3)
- **THEN** the discovery response SHALL report each environment's latest version distinctly, so consumers see skew before signing the contract

#### Scenario: Missing schema or domain

- **WHEN** a caller requests schemas for a domain that does not exist or is not `ACTIVE`, or a schema name absent from every environment
- **THEN** the API SHALL return `404 Not Found` with an error message identifying the missing entity

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

### Requirement: Discovery is read-only in this capability version

The schema-management capability SHALL provide schema creation and append-only version registration only through the publisher-authorized operations defined in this capability. It SHALL NOT provide publisher operations for schema replacement, retirement, version deletion, schema deletion, registry lifecycle, or registry policy administration. This change SHALL NOT alter the existing behavior in which any authenticated platform caller may discover any active domain's schemas. That discovery behavior is an interim baseline subject to the later `add-subscriber-entitlements` change and SHALL NOT be interpreted as the final combined visibility policy.

#### Scenario: Only append-only write operations exposed

- **WHEN** a publisher uses the Nexus schema API
- **THEN** the only schema write operations exposed SHALL be schema creation and appending a new schema version

#### Scenario: Discovery behavior is unchanged by publisher APIs

- **WHEN** any authenticated caller requests schema discovery for an active domain
- **THEN** this change SHALL preserve the pre-existing discovery response regardless of the caller's relationship to the domain
- **AND THEN** this interim requirement SHALL NOT constrain the final visibility behavior specified by the later `add-subscriber-entitlements` change
