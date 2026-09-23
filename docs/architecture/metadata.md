# Platform metadata composition

[Documentation home](../README.md)

Metadata revisions and business schema versions change independently. For activation steps, use the separately authorized [rollout runbook](../operations/metadata-rollout.md).

## Platform metadata composition

Every admitted event carries a platform-generated metadata block alongside the publisher's business `data`. The two components version **independently**: an exact contract is `(organisation, environment, schemaName, businessVersion, metadataRevision)`—metadata revisions never move business versions, and business schema appends never touch metadata. `konductor-metadata-configuration` stores immutable additive revisions (protected defaults `schemaVersion`, `idempotencyKey`, `publishedAt`, `metadataRevision` plus generated extension fields from a bounded generator catalog) and per-environment active pointers. Revision 0 is the read-only legacy selector for events predating metadata stamping.

After authorization, admission reads the environment pointer once with strong consistency. This read selects the event's metadata revision. A request that reads before an activation commit might complete under the old revision. A request that reads after the commit gets the new revision. There is no atomic transaction spanning DynamoDB and SQS.

Admission freezes generated values into a **format-2 queue envelope**. It carries the business and schema identity, target, metadata revision and digest, compiled metadata schema or bounded descriptor, and generated extension values. Delivery needs no configuration reads, so retries retain the recorded values.

The exception is `publishedAt`, the SQS acceptance time. Preflight uses its fixed-width representation because the value is not yet known. No configurable constraint can depend on that eventual value. Delivery derives it from the recorded queue message.

Format-2 messages with missing or inconsistent fields fail closed. Only actual legacy wrappers use the old three-field path. A retry after an ambiguous enqueue is a new admission. The UUID does not guarantee exactly-once processing.

Metadata administration is OAuth-only (`/metadata/*`, platform roles): additive revision creation with stale-head conflict detection (409), non-persisting preview/readiness, and per-environment activation with explicit expected-current conditions, forward-only moves, server-side rechecks, and an append-only activation audit. Both write paths are stream-native—one conditional transaction commits the record plus its mutation audit, and the `MetadataConfigurationTable` stream consumer emits `MetadataRevisionCreated`/`MetadataRevisionActivated` from committed images with stream-event-id marker dedup (full records in the [decision log](../decisions/README.md)). Activation deliberately imposes no dev/staging/prod ordering; each environment moves explicitly, and skipping intermediate additive revisions is valid.

Composed contracts are discoverable without mutating anything: `GET /schemas/{schemaName}/contract` returns the exact current pair or an **explicitly selected** historical pair (both `schemaVersion` and `metadataRevision` required together—the active revision is never substituted for a historical business version), with raw business content, the generated metadata schema, generator descriptions, and an offline-usable composed draft-4 document. `POST /schemas/contract-preview` composes **unsaved** business content for publishers without registering. Catalog summaries carry the active metadata revision per environment; the raw registry documents remain business-only. Provenance remains the event's own recorded tuple—a historical composition is inspection, not evidence the pair was ever emitted.

## Deep dives

[Publish validation](publish-validation.md#9-metadata-in-the-admitted-command-format-2) owns queue-format and delivery invariants. The [metadata decision](../decisions/add-platform-metadata-composition.md) records alternatives and cache-cutover trade-offs.
