# Publish write paths

[Documentation home](../../README.md)

Dated records from the [side-effects audit](README.md). These are rationale and failure analysis, not proof that the present code still has this shape.

## `publish-events`

- **Committed writes:** none to DynamoDB—the EventBridge `PutEvents` to the domain bus is the only commit.
- **Side effects:** the PutEvents itself; envelope metadata derives entirely from the immutable SQS message attributes (`IdempotencyKey`, `SentTimestamp`, admission-stamped `SchemaVersion`, `Environment`). Since `validate-at-ingest` the consumer performs no authorization, no registry reads, and no payload validation—admission owned those synchronously; the domain-ACTIVE recheck remains as the only live read.
- **Rubric:** (6) yes, by design—the inbound SQS command is the durable envelope; this is the rubric's "side effect is the purpose" case.
- **Failure walk:** crash after a successful PutEvents → redelivery publishes a duplicate; consumers dedupe on the carried `idempotencyKey`. Accepted at-least-once semantics.
- **Decision:** ENVELOPE-DRIVEN. **Rationale:** already the target shape.

## `publish-events-admission`

- **Committed writes:** none to DynamoDB—the SQS `SendMessage` to the publish queue is the only commit, and it is the durability boundary the 202 response reports.
- **Side effects:** the SendMessage itself; everything before it (authorization, domain-ACTIVE check, schema resolve via in-memory cache, draft-4 validation, refresh-before-reject) is read-only and synchronous.
- **Rubric:** (4) no—synchronous request/response path, exempt by the rubric's own terms (the response must reflect the outcome of validation); (6) yes for the downstream consumer—the enqueued command is the durable envelope.
- **Failure walk:** crash before SendMessage → the caller saw no 202 and owns the event; retry is safe (idempotency key derives from the caller's key or the API Gateway request ID). Ambiguous SendMessage failure (timeout after the write) → 503 to the caller, possible queued duplicate on retry; same at-least-once semantics as the consumer's duplicate window.
- **Decision:** STAY. **Rationale:** single atomic service call after read-only gates; nothing to stream.

## `schema-admin`

> **Moved (2026-02-11, `promote-schema-versions`):** the current record, including the version-content record write and conflict self-heal, lives in [schemas.md](schemas.md). The pre-promotion record below is preserved as history.

- **Committed writes:** none to DynamoDB—`CreateSchema`/`UpdateSchema` against the schemas service is the only mutation.
- **Side effects:** `SchemaVersionPublished` on the control-plane bus after every committed registry write (added by `follow-schema-promotion`, 2026-09-08), carrying `{producerDomain, schemaName, environment, version, publishedAt}`; before that change, none.
- **Rubric:** (4) no—synchronous read-your-write paths are exempt by the rubric's own terms. The promotion-signal emission takes a documented exception to (2): the registry write IS the committed transition, but there is no DynamoDB stream off the registry, so the event cannot be stream-derived. The exception holds because the promotion extender tolerates a lost signal (extension is idempotent; the next publish to that environment re-triggers it) and because appends are not safely retryable—surfacing an emission failure as a 5xx would invite a duplicate-version retry, a worse failure than a missed signal. Emission failure is therefore logged-only (`tracing::error!` with schema and cause), never surfaced.
- **Failure walk:** client retry after an unacknowledged create → 409 (safe); retry of an update appends a duplicate version—accepted, schemas are append-only. Registry write commits but emission fails → subscriptions awaiting that environment stay `AWAITING_PROMOTION` until the next publish to it (or a manual re-subscribe). Registry write fails → no event; nothing committed.
- **Decision:** STAY. **Rationale:** single atomic service call after read-only gates; the promotion signal hangs on the committed registry write with the logged-only exception above.
