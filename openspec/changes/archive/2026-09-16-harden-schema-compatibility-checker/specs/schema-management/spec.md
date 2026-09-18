## MODIFIED Requirements

### Requirement: Flat append-only schema version API

The separate OAuth and IAM control-plane surfaces SHALL each provide `POST /schemas/versions` as a flat verb endpoint for appending a schema version. The request SHALL accept `domain`, `eventName`, and `content` in the request body. The target environment SHALL be inferred from the API Gateway stage. Both surfaces SHALL normalize their authenticated principal and require the grant engine to allow the publisher action for that domain. Konductor SHALL derive the canonical schema name and SHALL use the AWS Schemas version-producing operation so existing versions remain immutable. The appended `content` SHALL parse as a valid JSON Schema draft-4 document, and SHALL be rejected with `400 Bad Request` otherwise.

Before writing, Konductor SHALL read the current latest version in the stage-selected registry as the comparison predecessor. An accepted candidate SHALL preserve every payload accepted by that predecessor under the application's supported Draft-4 validation semantics. Acceptance SHALL require a sound proof within the supported comparison rules and bounded comparison work; unsupported interactions, precision-sensitive comparisons without a sound rule, and exhausted proof budgets SHALL fail closed with `400 Bad Request` and no registry write. Compatibility SHALL be judged against the predecessor read for this append, never historical versions chosen by the caller. This pairwise check SHALL NOT be represented as serialization of concurrent appends or as protection against a competing write between predecessor lookup and registry update.

New optional properties SHALL NOT be accepted solely because they are optional: their constraints SHALL preserve values the predecessor allowed for those property names, including applicable additional-property and pattern-property constraints. In the supported reference-free domain, adding an optional property to an object that previously prohibited that name SHALL be accepted when the other constraints remain compatible. Adding a new constraint to a previously unrestricted property name SHALL be rejected unless acceptance of all previously allowed values can be established.

The gate SHALL preserve supported proofs for identical documents, dropped `required` entries, type/enum widening, and loosened or removed constraints, subject to the proof budget and applicable interaction checks. Changed schema-bearing references, reference-dependent changes, and other unsupported semantics SHALL NOT be accepted merely because local keywords appear unchanged or loosened. Numeric containment SHALL NOT be inferred from lossy conversion or rounded arithmetic. The gate SHALL continue to conservatively reject removed declared properties and other unproven changes; it SHALL NOT imply support for complete semantic implication checking.

Rejections SHALL identify relevant paths and reasons using the existing validation-error response envelope, with at most ten reported paths and an indication when more reasons exist. Explanations SHALL distinguish local constraint restrictions from unsupported proof cases and SHALL NOT describe a change as demonstrably incompatible solely because proof was unavailable. The gate SHALL remain absolute: no request flag, role, or binding SHALL bypass it. Intentional breaking evolution SHALL be expressed as a new schema under a new event name, never as a new version. Schema creation (`POST /schemas`) SHALL NOT be compatibility-gated because version 1 has no predecessor. Publisher APIs SHALL NOT expose schema deletion, retirement, version deletion, or historical-version replacement.

#### Scenario: Authorized publisher appends a version

- **WHEN** a caller with a `publisher` binding for domain `orders` appends a supported provably compatible schema document for event `OrderNotification` on the `staging` stage
- **THEN** Konductor SHALL create the next version of `{namespaceIdentity}@OrderNotification` in `{org}-staging-orders-schema-registry`
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
