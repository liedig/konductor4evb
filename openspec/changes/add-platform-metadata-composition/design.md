## Context

See `proposal.md` for motivation and the four delta specs for behavior.

Observed integration points:

- `lib/konductor-common/src/envelope.rs` fixes three metadata fields and checks envelope structure/size. `publishedAt` is derived from SQS `SentTimestamp`.
- `schema_validation.rs` compiles draft-4 business documents and maintains a latest-schema TTL cache. `publish-events-admission` validates `PublishRequest.data`, then queues admitted attributes. `publish-events` reads those attributes without resolving schemas.
- `schema-admin` stores publisher business content unchanged in per-environment EventBridge registries. `describe-schema` and `list-schemas` enforce catalog visibility. Historical business content is accessible by registry version, but there is no composed-contract endpoint.
- `gate-schema-promotion` is a separate, mostly unimplemented plan for business-schema compatibility. Its conservative checker freezes `$ref`/combinator subtrees, so passing a newly composed envelope to that checker would falsely reject metadata updates.
- `SchemaAuthoringForm.vue` accepts business content and infers its environment from the configured API stage. The new administration page needs explicit environment selection without changing that existing authoring behavior.

This design is needed because the change crosses validation, immutable queue messages, configuration state, security, and the console.

## Goals / Non-Goals

**Goals**

- Keep exact historical validation reproducible while the current metadata pointer advances.
- Make generator definitions declarative, bounded, and safe; avoid a general-purpose policy language.
- Give each administration mutation one synchronous durable commit and put lifecycle emissions behind its stream.
- Preserve current business registry identities, subscription pins, publisher request shape, and queue durability.

**Non-Goals**

- General customer setup, identity/group configuration, or arbitrary configuration precedence.
- Automatically copying business schema versions between environments or introducing an approval workflow.
- Exactly-once delivery, producer-request deduplication, per-domain metadata exceptions, or rewriting events.
- A platform-administrator escape hatch for breaking metadata changes.

## Decisions

### 1. Independent immutable versions, not registry fan-out

An exact contract is `(organisation, environment, schemaName, businessVersion, metadataRevision)`. `schemaVersion` retains its routing meaning; `metadataRevision` is a separate positive integer. The full tuple is necessary because the same registry version number may describe different business content in different environments.

Do not append every business registry schema when metadata changes. Store metadata revisions independently and expose the composed contract through Konductor. Native registry reads still return raw business schemas. Current discovery explicitly resolves the current pair; historical discovery requires both version selectors.

Revision 0 is a read-only legacy selector for events lacking a metadata stamp. Revision 1 contains protected defaults plus the new discriminator. Subsequent revisions add fields. Absence is interpreted as legacy only for the old queue/event format, never as recovery from a malformed new-format message.

An explicit historical request can compose immutable component versions; it is not evidence that this combination was ever emitted. Event provenance is the actual event's recorded tuple, not an invented mapping from every business version to one metadata revision.

Alternative rejected: mutable shared `$ref` to “latest metadata”, which would change historical validation. A single version for both components was also rejected because configuration edits would move business versions and disrupt subscriptions.

### 2. Small generator catalogue and typed definition tree

Model definitions as objects with named children or generated leaf fields. Initial supported leaves:

| Generator | Generated type and constraint | Source |
|---|---|---|
| `uuid-v4` | String, UUID-v4 shape | Platform RNG, once per admitted event |
| `utc-timestamp` | UTC RFC3339 string with milliseconds | One captured admission instant |
| `utc-date` | UTC `YYYY-MM-DD` string | The same captured admission instant |
| `aws-region` | Nonempty string | Trusted processing region |
| `organisation` | Nonempty string | Resolved platform organisation |
| `environment` | Nonempty string | Authorized publish API stage |

Types and primitive validation constraints are derived from generators, not arbitrary user-provided JSON Schema. The UI exposes these constraints read-only. This avoids accepting a UUID regex or date bound that works in preview but can fail for legitimate future generated values. No caller-supplied values, secrets, AWS SDK lookups, arbitrary strings evaluated as expressions, scripts, arrays, or remote references are part of the initial generator model.

Use field segments matching `[A-Za-z][A-Za-z0-9_]{0,63}`; reject reserved built-in names at the metadata root and unsafe JavaScript object keys at every level. Proposed initial limits: 64 total extension nodes, four object levels, and a 16 KiB canonical definition. Enforce identical limits server-side and show them in the editor. These are implementation defaults to test, not permission to relax payload limits.

The additive checker compares definition trees, requiring exact preservation of existing leaf descriptors and object kinds while allowing new child keys. Schema `required` arrays are generated from each revision's fields; do not compare these arrays with the business-schema loosening checker. Saved revisions are immutable; edits are local drafts until saved as a new revision against the current global head.

Protected defaults retain their current semantics. `idempotencyKey` remains the explicit supported publisher input exception. `publishedAt` remains queue time, not an alias of the new admission-time generator.

### 3. Configuration storage, concurrency, and administration APIs

Add an organisation-scoped metadata table, separate from domains and not a schema catalog. Use partitioned item types for immutable revisions, the global revision head, per-environment active pointers, and immutable mutation/audit records. Do not put all hot environment reads behind one organisation-wide partition key.

- Revision records contain predecessor, canonical definition, generated metadata schema, definition digest, compiler format version, actor, and creation time. No TTL/deletion.
- Environment records contain active revision, definition digest, generation, and activation audit identity.
- Audit records contain the operation, actor, target environment where applicable, before/after revision identities, request identity, and time. No business payload or authentication material.

One `TransactWriteItems` per mutation conditionally updates the head/pointer and creates its immutable revision/audit record. Requests carry expected predecessor/current revision and an operation id. Reusing an operation id with identical content returns the recorded outcome; different content conflicts. A concurrent save/activation cannot silently overwrite another administrator. Reapplying the active revision is a no-op; backward activation is rejected.

Proposed OAuth-only routes:

| Route | Behavior |
|---|---|
| `GET /metadata/revisions` | Paginated revision summaries |
| `GET /metadata/revisions/{revision}` | Definition and exact generated metadata schema |
| `POST /metadata/revisions` | Create revision against expected head |
| `POST /metadata/preview` | Validate draft and return synthetic example, no persistence |
| `GET /metadata/environments` | Configured environments and active revisions |
| `POST /metadata/readiness` | Read-only validation of a saved revision in a selected environment |
| `POST /metadata/activations` | Activate revision for explicit environment and expected current state |
| `GET /metadata/activations` | Paginated activation audit history |

Reads and read-only previews permit controlled platform roles; mutations require `platform-admin` independent of domain bindings. No management route is added to the IAM surface. Treat this as an intentional API-surface exception, like binding administration, and test it.

Validate environment names against deployment configuration, not user text alone. Selecting an environment in these administration requests is intentional and distinct from the publish/schema-write stage selector. Existing configured inputs suffice; no dependency on implementing `configure-customer-platform`.

Use established errors: 400 invalid definition/environment, 403 authorization, 404 unknown revision, 409 stale state or incompatible activation direction, 503 unavailable configuration services. Invalid additive edits are 400 with bounded field paths.

### 4. Precise activation boundary and cache behavior

The active pointer transaction is the cutover. After authorization, each admission strongly reads its environment pointer once. That read is the event's metadata selection boundary. Requests that read before the commit may complete afterward under the old revision; requests reading afterward get the new revision. Do not promise queue-time atomicity between DynamoDB and SQS or invalidate already in-flight work.

Immutable revision contents can be cached indefinitely by revision and digest with bounded memory. Never TTL-cache the mutable active pointer. Missing/unreadable state is 503, not fallback to an older cached revision.

Include metadata generation in the latest-business-schema cache key. On activation, old latest entries cannot be reused in the new generation; the first publish per schema/instance refreshes the registry. Within a generation retain the existing maximum 60-second business freshness and refresh-before-reject behavior. This deliberately does not strengthen business-schema append consistency; “current business schema” retains that existing bounded-cache meaning. The metadata boundary itself has no TTL lag.

If refresh-before-reject selects a newer business version, rebuild the exact composition and size check while retaining the already selected metadata revision. Generate UUIDs only for the final selected admission attempt.

Readiness validates the immutable metadata definition, generated constraints, target context, size budget, and the versioned composition format. Metadata and data occupy disjoint schema scopes, so it does not need an atomic rewrite or scan of all registries. The UI separately pages the affected current-schema inventory and allows previews. That inventory is an observation, not a snapshot lock or proof of every external consumer's readiness. Activation re-runs authoritative checks and compares expected pointer state; it does not trust browser preflight results.

No automatic dev/staging/prod ordering is imposed. The administrator explicitly activates per environment; each move must extend that environment's current revision. Definitions form a linear additive history, so skipping intermediate additive revisions is valid.

### 5. Exact composition and historical discovery

Add `GET /schemas/{schemaName}/contract` on both surfaces with explicit `environment` and either current selection or both `schemaVersion` and `metadataRevision`. Reuse existing producer-domain/consumer-domain authorization and name resolution. Historical reads require current authorization, not historical entitlements. Unknown components return 404; incomplete/invalid selectors return 400.

Return the tuple, component digests, raw business content, generated metadata schema, generator descriptions, and an offline-usable composed draft-4 document. Existing raw detail `content` remains business-only. List summaries may include active metadata revisions, without becoming a persisted schema index.

The composed document validates the `detail` object: required `metadata` and `data`, metadata restricted to the exact revision and stamped numeric versions, and business data validated in its original schema scope. Add `POST /schemas/contract-preview` on both surfaces for publisher-authorized, unsaved business-content preview; it does not register schemas or grant additional catalog visibility.

Do not naively paste business JSON below `properties.data`: a root-local `$ref` could then refer to the envelope. Bundle the business document as a distinct embedded draft-4 resource, preserving existing `id` scopes or assigning a deterministic internal identifier when absent. All references must resolve inside the exported bundle using local resource registration; no runtime network resolver. Preserve recursion and local definition semantics. Test this with independent offline compilation as well as the admission validator. Unresolvable legacy external references are a compatibility failure to report before rollout, not permission to fetch arbitrary URLs.

Composed validators cache by registry identity, business version/content digest, metadata revision/digest, and composer format version. Persist the metadata schema/compiler format with each revision so a future compiler change cannot silently reinterpret an old definition. Retain old compiler formats or introduce an explicit new format, with golden historical fixtures.

Metadata exact schemas require all configured fields and reject unknown fields. They are intended for the event's stamped revision, not as a permanently pinned validator for every future event. Consumers must either resolve the exact recorded revision or adopt an explicit extension-tolerant metadata reader. Keeping `additionalProperties` open alone is not sufficient if a consumer also insists on the old `metadataRevision` value.

### 6. Admission, queue format, and the queue-time exception

New admissions use an explicitly versioned queue envelope (format 2), containing the supported publisher body, resolved business/schema identity, target, metadata revision/digest, and generated extension values. Store metadata in the body rather than adding an unbounded set of SQS attributes. Preserve existing trusted principal attributes and the queue policy that limits sends to admission.

Pipeline:

```text
authorize + active domain
  --> strongly resolve metadata pointer
  --> resolve/validate latest business data (generation-scoped cache)
  --> generate extensions once from trusted context
  --> validate composed detail + exact final size
  --> SQS durable acceptance --> 202
  --> delivery materializes publishedAt --> PutEvents
```

Before enqueue, all generated fields except `publishedAt` are actual frozen values. For that reserved field, use the existing fixed-width UTC-millisecond timestamp representation for preflight validation/size and permit no configurable constraint that depends on its yet-unknown value. The supported deterministic timestamp conversion is checked at delivery against the frozen built-in rule. Thus there is no promise to know SQS's acceptance time before SQS accepts.

Delivery uses the recorded source/target and values, not current configuration, to avoid drift on retries. It checks queue-format integrity and immutable-field consistency and uses the recorded compiled metadata schema or bounded validation descriptor carried with the queue message to validate the late-bound timestamp and frozen fields without configuration reads. Include that descriptor in SQS message-size checks, but do not publish it to EventBridge. Business data is not revalidated against latest at delivery.

Keep domain-liveness checks and existing partial-batch failure handling. Format 2 with missing/inconsistent fields fails processing; only an actual legacy wrapper takes the old three-field path. No consumer path generates missing UUIDs or reads current metadata to repair a message.

A client retry after an ambiguous enqueue outcome is a new admission and may generate another UUID even with the same idempotency key. The application deduplication key remains `idempotencyKey`; UUID metadata is not an exactly-once guarantee. Existing documentation/spec claims that every non-202 proves nothing queued are stronger than distributed enqueue can guarantee; do not repeat that claim or broaden this change into retry deduplication.

### 7. Business-schema gate integration

Require the `gate-schema-promotion` behavior before enabling production metadata rollout, without editing its artifacts in this change. Keep its comparisons on publisher business documents. Add regression tests showing that tightening business data still rejects and metadata activation invokes no business append or `SchemaVersionPublished`.

At business schema create/append, check offline composability with the supported metadata wrapper as well as standalone draft-4 compilation. This is a representation-safety check, not a metadata-driven version bump or a bypass of the existing gate. Exact historical business content remains untouched.

Adding a required generated metadata field is intentionally not “old instance accepted by new schema”: M1 and M2 are different exact contracts, and admission supplies the new field. This must remain distinct from the business gate's previous-payload-validity guarantee.

### 8. Stream-driven side-effects classification

Revision creation and activation handlers each perform exactly one synchronous configuration transaction. That transaction includes everything an audit/event consumer needs: revision identity/digest, target, actor, operation identity, and before/after state.

- Lifecycle emission hangs only on committed immutable mutation records.
- A table stream consumer emits `MetadataRevisionCreated` and `MetadataRevisionActivated` with deterministic mutation identity. Duplicate delivery does not mutate configuration again; downstream consumers deduplicate that identity.
- Seconds of stream latency are acceptable because admission reads the committed pointer, not the emitted event.
- Stream processing needs partial failures, bounded retries, durable failed-record capture, iterator-age alarms, and a recovery runbook that meets the 24-hour retention bar. Permanent audit records allow recovery beyond the stream window.
- There is no inbound durable command for configuration mutations; the transaction/audit record provides durability. Preview/readiness do not persist or emit lifecycle events.
- Publish admission retains its existing single SQS commit and does not add a DynamoDB write per event.

Record these handler classifications in `docs/development/decisions.md` during implementation. Do not add direct EventBridge emission after a handler's DynamoDB commit.

### 9. Console composition and permissions

Add `/admin/metadata` and a platform navigation entry. Build small components using the current `NxCard`, inputs/selects, tables, modal/confirm, badges, and toasts, rather than introducing a raw JSON-only administration experience.

- `MetadataDefaultsPanel`: locked defaults with type/source explanations and the idempotency exception.
- `MetadataDefinitionEditor`: additive object/field tree, supported-generator selector, derived constraints, bounds and errors.
- `MetadataPreviewPanel`: synthetic example, metadata schema, composed business example, and clear “not published” label.
- `MetadataRevisionHistory`: paginated immutable revisions, diffs, author/time.
- `MetadataEnvironmentPanel`: active revision per environment, server readiness, timestamped paginated schema inventory, and activation audit history.
- `MetadataActivationConfirm`: environment/current/candidate identity, consumer-readiness acknowledgement, expected-state submission, busy-safe confirmation and conflict refresh.

Use a thin `useMetadata` composable over `useApi` and shared API types. Keep draft edits local and preserve them on errors; do not persist credentials, business payload examples, or unsaved drafts into configuration audit logs.

Extend `DomainSchemasPanel` and catalog detail with independent per-environment version badges. On the catalog detail view the environment indicators are selectable links that drive one contract read per selection, rendering the raw business schema (“Schema content”) and the composed document (“Schema preview”) side by side from the same environment. Schema authoring still submits only business content to its stage, and its effective-contract preview is labelled provisional/unregistered. The console exposes the environment's current pair only — no historical selectors at this stage; historical inspection stays an API capability addressed by the event's recorded tuple, with no “latest” substitution for an old event.

Auditors see read-only history and environment status; platform admins see mutations; domain users see only authorized composed contracts. Test server authorization as well as hidden controls. Browser tests use synthetic configuration/API data and cannot call live publish or management endpoints.

## Risks / Trade-offs

- **Strong read on every admission:** adds latency, RCU cost, and potential hot-key pressure at the project's event volume. Use small environment pointer items and bounded immutable caches; load-test representative volume and quantify cost before rollout. Throttling fails closed. Do not silently substitute a TTL pointer cache to meet a benchmark.
- **Cold composed generation after activation:** new registry lookups can throttle. Bound concurrency, preserve 503/retry behavior, measure activation cold-start bursts, and activate lower environments first operationally.
- **Closed-envelope consumers:** metadata additions can break consumer deserializers/validators even when domain data is unchanged. Require explicit readiness acknowledgement and a baseline rollout rehearsal. The UI must not claim automatic consumer compatibility.
- **Draft-4 resource scopes:** naïve embedding silently misvalidates references. Differential and offline tests are release gates, not optional follow-up work.
- **Monotonic additive growth:** field limits eventually constrain additions and incorrect generator choices cannot be edited. Preview and diff before saving; any future breaking migration requires a separate design.
- **Existing latest-business cache/concurrent append behavior:** preserved within a generation. The separate gate documents its own concurrency limitations; this change does not claim to solve them.
- **Operational recovery:** reverting a metadata pointer would remove fields from new events and violate the additive contract. Pause admissions if necessary and fix forward; do not offer an unsafe backward activation button.

## Migration Plan

1. Land/reconcile the business gate, offline-composition checks, and the configuration table/APIs before enabling new-format publishing. Add a deployment-time bootstrap that conditionally seeds revision 1 and each existing environment at legacy revision 0 without overwriting existing state on retries or redeployment.
2. Deploy a dual-format delivery consumer first. While legacy 0 is active, admission continues producing legacy format; unsupported active revisions fail closed. Verify historical fixture delivery and original timestamp/idempotency semantics.
3. Deploy admission and UI support while keeping environments on legacy 0. Inventory/compile current business schemas offline, check local-reference behavior, validate quotas and latency, and review real consumer extension tolerance. Unsupported historical reference patterns must be resolved explicitly before production activation.
4. Administrators activate baseline revision 1 in selected environments after readiness checks. Each environment now stamps `metadataRevision`; subsequent saved revisions add optional-to-configure but mandatory-when-active generated fields.
5. Exercise activation races, configuration outage, delayed queue messages, partial delivery failure, and replay in a sandbox. Rollout is separately authorized; planning/testing does not permit live service writes.
6. Rollback before any activation can leave legacy 0 untouched. After activation, do not deploy an old consumer that cannot read format 2, delete revisions, or move pointers backward. Keep a format-compatible build available; pause new admissions for an incident and fix forward while preserving queued events and historical retrieval.
