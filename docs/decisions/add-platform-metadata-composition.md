# Independent platform metadata contracts

[Documentation home](../README.md)

- Recorded date: 2026-09-22
- Change ID: `add-platform-metadata-composition`
- Status: recorded design decisions; rollout is separately authorized.

## Decision and rationale

The platform metadata capability was authored under the AGENTS.md rubric, so every new handler write path is classified here at authoring time (the 2026-08-25 audit above remains a frozen snapshot of the pre-metadata codebase). Three cross-cutting decisions frame the records; per-handler classifications follow.

**Independent immutable versions (design decision 1).** An exact metadata contract is the five-part tuple `(organisation, environment, schemaName, businessVersion, metadataRevision)`; `metadataRevision` is a separate positive integer because the same registry version number can describe different business content in different environments. Metadata revisions are stored independently in `konductor-metadata-configuration`—the registry is never fanned out with a copy of every composed schema, and native registry reads still return raw business documents. Revision 0 is a read-only legacy selector for events predating metadata stamping; absence is legacy only for the old queue/event format, never recovery from a malformed new-format message. An explicit historical request composes immutable component versions and is not evidence that combination was ever emitted—event provenance is the event's own recorded tuple. Mutable shared `$ref`-to-latest and a single shared version for both components were both rejected (they would change historical validation or make configuration edits move business versions).

**The queue-time exception (design decision 6).** Every generated metadata field is frozen before enqueue except `publishedAt`, the Amazon Simple Queue Service (Amazon SQS) acceptance time. Preflight uses the fixed-width UTC-millisecond representation because the value is not yet known. No configurable constraint can depend on its eventual value.

Delivery derives the timestamp from the recorded queue message. It validates frozen fields against the recorded compiled metadata schema or bounded descriptor in the format-2 envelope. It neither reads configuration nor repairs missing universally unique identifiers (UUIDs), and it never revalidates against the latest revision.

Format-2 messages with missing or inconsistent fields fail closed. Only actual legacy wrappers use the old three-field path. A client retry after an ambiguous enqueue outcome is a new admission. It might generate another UUID even with the same idempotency key. UUID metadata does not guarantee exactly-once processing, and a non-`202` response does not always prove that nothing was queued.

**Cache-cutover costs (design decision 4).** The environment-pointer transaction is the cutover: after authorization each admission strongly reads its environment pointer once, and that read is the event's metadata selection boundary—requests reading before the commit may complete afterward under the old revision, and no cross-store (DynamoDB↔SQS) atomicity is promised. Immutable revision contents cache indefinitely keyed by revision and digest with bounded memory; the mutable active pointer is never TTL-cached, and missing/unreadable configuration is 503 with no fallback to an older cached revision. The metadata generation joins the latest-business-schema cache key, so an activation makes old latest entries unusable and the first publish per schema refreshes the registry—within a generation the existing ≤60 s business freshness and refresh-before-reject behavior are retained deliberately. If refresh-before-reject selects a newer business version, the composition and size check are rebuilt while retaining the already-selected metadata revision, and UUIDs are generated only for the final selected admission attempt.

## Evidence and related topics

- [OpenSpec design](../../openspec/changes/archive/2026-09-11-add-platform-metadata-composition/design.md)
- [Metadata architecture](../architecture/metadata.md)
- [Write-path classifications](../development/side-effects/metadata.md)
- [Rollout runbook](../operations/metadata-rollout.md)

[Decision index](README.md)
