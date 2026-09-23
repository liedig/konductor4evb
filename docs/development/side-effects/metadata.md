# Metadata write paths

[Documentation home](../../README.md)

Records dated 2026-09-22 for `add-platform-metadata-composition`. Read the [rationale](../../decisions/add-platform-metadata-composition.md) and [rubric](README.md).

## Metadata write-path classifications

The following records classify each metadata path against the durability rubric.

| Call site | Decision | Rationale (abbreviated rubric answers) |
|---|---|---|
| `metadata-admin/create.rs`—revision creation | STREAM-NATIVE (compliant) | One `TransactWriteItems` commits the revision record, the mutation audit record, and the conditional head advance together; no other side effect exists in the handler—lifecycle emission is the stream consumer's job. |
| `metadata-admin/activation.rs`—environment activation | STREAM-NATIVE (compliant) | One `TransactWriteItems` moves the pointer (conditioned on the expected current revision, forward-only) and commits the audit record (`attribute_not_exists` PK); readiness is rechecked server-side from committed state, never trusting browser preflight. |
| `metadata-admin` preview/readiness/environments reads | No write path | Non-persisting by construction; verified by SDK-mock tests that no state mutation or publish occurs. |
| `control-plane/metadata-stream-consumer` | Reference (exempt) | The rubric's intended consumer: emits only from committed stream images, claims each stream event id with a TTL'd marker for idempotence, releases the marker on emission failure so bounded retries can re-claim, captures failures durably, and relies on permanent audit records for recovery beyond the stream window. |
| `publish-events-admission` | STAY | Retains its existing single synchronous commit—the SQS enqueue. Metadata generation added pointer resolution and composition to the pre-commit validation path but no per-event DynamoDB write; 202 remains the only commit point. |
| `publish-events`—delivery | ENVELOPE-DRIVEN | The format-2 queue message is the durable envelope: delivery reads recorded source/target/values and the carried compiled schema, performs no configuration reads, and retries cannot drift because values are frozen at admission. |
| `describe-schema`—contract and preview routes | No write path | Read-only composition over stored components; preview composes unsaved business content and registers/publishes nothing. |
| `schema-admin`—create/append | STAY | Existing synchronous registry write plus a new pre-write offline composability check; the check is pure validation (fail-fast before any SDK call), not a side effect, and exact historical business content is untouched. |

**Verification (2026-09-22):** both metadata transactions carry everything the stream consumer needs on the committed image—revision identity, definition digest, target, actor, operation id, and before/after revisions (`MutationAuditRecord` rides inside each transaction, so the audit trail cannot diverge from configuration state); the consumer's tests (task 3.5) prove only committed records emit and duplicate delivery preserves deterministic identity; activation tests (task 3.4) prove the server reruns authoritative checks rather than trusting client readiness results. No handler-driven path was retained: metadata mutations have no inbound durable command, and the transaction/audit record itself is the durability source, per design decision 8.
