## Why

Issue #30 assumes the existing syntactic gate is a safe fast path before broader semantic checks, but the current checker can accept schema pairs that reject previously valid payloads. Optional-property constraints and changes to indirectly referenced subschemas demonstrate that correctness must precede broader acceptance.

## What Changes

- **BREAKING**: replace unconditional acceptance of new optional properties with proof against the predecessor's effective property constraints, including its additional-property policy.
- Guard reference-dependent changes: unchanged reference text does not establish unchanged meaning. Unsupported reference interactions fail closed instead of entering the current acceptance fast path.
- Align numeric reasoning with the actual Draft-4 validation configuration; do not infer containment from lossy conversions or rounded division.
- Make rejection explanations distinguish observed constraint changes from unavailable compatibility proofs, without claiming every unproven change is semantically incompatible.
- Preserve supported provably safe evolution and no-op appends. Keep conservative rejection where proof is unavailable; do not introduce broad semantic acceptance in this change.
- Correct the existing schema-management requirement and producer/cache-safety documentation, and add Rust validator-backed regression coverage.
- Explicitly defer concurrent-append serialization. This change proves a candidate against the predecessor read by the handler, not against a version written concurrently after that read.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `schema-management`: make append acceptance depend on a sound bounded proof for the compared schema pair, correct the unconditional optional-property rule, and describe fail-closed unsupported cases accurately.

## Impact

- Shared checker: `lib/konductor-common/src/schema_compatibility/`, with the existing schema validation/composition paths as the semantic oracle.
- Handler and tests: `event-streams/functions/schema-admin/src/handler.rs`; retain authorization ordering, latest-version lookup, HTTP 400 envelope, capped paths, and no writes or publication events on rejection.
- API documentation: `api/openapi-oauth.yaml` and `api/openapi-iam.yaml`.
- Guides: `docs/publishing-events.md`, `docs/development/publish-validation.md`, `docs/development/decisions.md`, and the schema-gate description in `docs/development/architecture.md`.
- No solver service, new AWS resource, append lock, dependency upgrade, deployment, or change to schema creation and subscription pinning.
- Follow-ups remain separate: bounded acceptance improvements and broader semantic reasoning under issue #30; concurrency safety requires its own change. No new GitHub issue or comment is authorized by this planning capture.
