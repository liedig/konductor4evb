## MODIFIED Requirements

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
