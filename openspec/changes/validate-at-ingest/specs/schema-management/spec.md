## MODIFIED Requirements

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

The separate OAuth and IAM control-plane surfaces SHALL each provide `POST /schemas/versions` as a flat verb endpoint for appending a schema version. The request SHALL accept `domain`, `eventName`, and `content` in the request body. The target environment SHALL be inferred from the API Gateway stage. Both surfaces SHALL normalize their authenticated principal and require the grant engine to allow the publisher action for that domain. Konductor SHALL derive the canonical schema name and SHALL use the AWS Schemas version-producing operation so existing versions remain immutable. The appended `content` SHALL parse as a valid JSON Schema draft-4 document, and SHALL be rejected with `400 Bad Request` otherwise. Publisher APIs SHALL NOT expose schema deletion, retirement, version deletion, or historical-version replacement.

#### Scenario: Authorized publisher appends a version

- **WHEN** a caller with a `publisher` binding for domain `orders` appends a schema document for event `OrderNotification` on the `staging` stage
- **THEN** Konductor SHALL create the next version of `{namespaceIdentity}@OrderNotification` in `{org}-staging-orders-schema-registry`
- **AND THEN** all earlier versions SHALL remain unchanged
- **AND THEN** the API SHALL return `201 Created` with the schema name and new version

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
