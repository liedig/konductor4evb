# Admission-at-the-edge publish validation: synchronous `publish-events-admission` Lambda + delivery-only consumer; draft-4-only schemas

[Documentation home](../README.md)

- Recorded date: 2026-09-02
- Change ID: `validate-at-ingest`
- Area: Event Streams
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

**Admission/delivery split**: `/publish` moves from API-Gateway-direct-to-SQS to a synchronous `publish-events-admission` Lambda (aws_proxy) that authenticates identity, authorizes `schema:publish`, checks domain liveness, resolves the latest schema version (TTL-bounded cache ≤60 s with force-refresh before any 422), validates `data` against it (draft-4 pinned), and stamps the validated version into the `SchemaVersion` SQS message attribute before enqueue. 202 is the synchronous commit point; deterministic failures answer 403/404/422, transient ones 503.

**Earned stamp**: the delivery consumer (`publish-events`) performs no authorization, schema fetch, or payload validation—it stamps the admission-provided `SchemaVersion` verbatim; consumer IAM loses `schemas:DescribeSchema` and binding-table reads, and `PublishQueue` grants `sqs:SendMessage` to the admission role only.

**Draft-4-only**: `POST /schemas` and `/schemas/versions` reject non-`JSONSchemaDraft4` types and unparseable draft-4 content with 400, so nothing unvalidatable is ever stored (regression: a 503 forever).

**Refresh-before-reject**: a cached schema that rejects a payload is force-refreshed from the registry before answering 422—a brand-new append can never falsely reject.

**Registry authority invariant**: the registry is the only source of truth for latest-version conformance—never DynamoDB projection state.

**Rejected alternatives**: consumer-side validation (publisher learns of violations only by absence); API-Gateway request validators (request-shape only, no draft-4); schema-on-read (violations discovered at query time); validating against a pinned version (couples producers to consumer migration).

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-09-07-validate-at-ingest/proposal.md). The recorded date above is not necessarily the archive or merge date.

[Decision index](README.md)
