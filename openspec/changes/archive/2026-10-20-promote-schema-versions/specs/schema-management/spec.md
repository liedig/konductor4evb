# Spec Delta

## ADDED Requirements

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

## MODIFIED Requirements

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
