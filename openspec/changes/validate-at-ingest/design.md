# Design: Validate at Ingest

## Context

Today `POST /publish` is compute-free at the edge: API Gateway integrates directly with SQS via VTL, returns `202`, and the `publish-events` consumer Lambda performs authorization, schema resolution (`DescribeSchema` per publish), envelope construction, and `PutEvents`. See proposal.md for motivation: per-publish registry calls throttle at scale, the version stamp is unverified, and rejections surface only as post-hoc DLQ entries.

## Goals / Non-Goals

**Goals:**

- Every queued event is authorized, conformant to the latest in-environment schema, and stamped with the version it was validated against
- Synchronous `4xx` feedback for publisher errors; the queue carries only deliverable work
- Eliminate per-publish `DescribeSchema` calls (throttle ceiling at billions of events/month)
- Keep the consumer's retry semantics purely for transient delivery failures

**Non-Goals:**

- Schema promotion / release gating (parked: future `gate-schema-promotion` change; this design is compatible but does not depend on it)
- Publisher-declared schema versions in the publish contract (considered, rejected — see Decisions)
- Version overlap windows during rollout (considered, rejected — additive-only evolution norm makes them unnecessary)
- Publisher SDKs or a separate `POST /validate` dry-run endpoint (the dev-stage `POST /publish` is the dry run)
- Schema authoring UI (separate change: `schema-authoring-ui`)

## Decisions

### 1. Admission at the edge (API Gateway → validator Lambda → SQS)

A synchronous Lambda enters the ingest path and performs grant-engine authorization, schema resolution, payload validation, and version stamping before `SendMessage`. The publish consumer shrinks to delivery: read admitted attributes, build the `PutEvents` entry, retry transient failures.

*Alternative — keep post-queue validation:* rejected because the feedback loop stays closed (publisher already holds a `202`) and poison messages keep flowing to the DLQ, where they are indistinguishable from delivery failures.

*Trade-off accepted:* the ingest path now depends on Lambda availability. The durability boundary moves from API-call time to `202` time; pre-acceptance retry is the publisher's responsibility, which is the standard synchronous-API contract and is now explicit in the spec.

### 2. Validate against latest-in-force only; no declared version

The publish contract stays `domain`, `eventName`, `data`, `idempotencyKey`. Admission validates against the registry's latest version in the stage-selected environment.

*Alternative — publisher-declared version:* rejected. Declared-vs-latest mismatch muddies the water (is a v2 declaration a stale deployment or an error?), and under the additive-only evolution norm it buys nothing: an old build emitting the previous shape still validates against an additive successor. Breaking changes take the new-event-name escape hatch (`OrderNotificationV2` as a new schema), which needs no version machinery.

*Alternative — overlap windows (admit last two versions, route `[n-1, n]`):* rejected. It forces consumers to handle two shapes simultaneously and cannot distinguish "legit old shape" from "malformed new shape" without a declared version. Windows only matter for breaking changes, which the norm excludes.

### 3. In-memory cache with short TTL, refresh-before-reject

The validator caches `{latestVersion, content}` per `(environment, schema)` in execution-environment memory, TTL ≤ 60s. Registry reads happen on cold cache, TTL expiry, or the refresh-before-reject path — never per publish. Write-through invalidation was considered and rejected: the version-append path runs in a different Lambda (`schema-admin`) with no way to poke warm validator instances, and append traffic is rare.

The key correctness property: **on validation failure only, refresh once and re-validate before rejecting.** Failures are rare (steady state is valid publishes served from cache), so the extra `DescribeSchema` costs nothing in practice, while a `400` always means "fails the actual latest" — never "stale cache." This also fixes the dev-iteration hazard (append v3, immediately publish new shape) without any invalidation machinery.

Registry-read failure (including throttling) fails closed with `503`: an expired entry is never used to stamp, because stamping a possibly-superseded version would silently re-introduce the unearned-assertion problem.

*Alternative — DynamoDB-backed cache:* deferred. Adds a table and a propagation path for no measurable gain at current scale; the in-memory design admits it later without spec change.

### 4. JSON Schema draft 4 only, enforced at creation

`POST /schemas` rejects `OpenApi3` with `400`; both create and append validate that `content` parses as draft 4. Rationale: an OpenAPI 3.0 document describes an HTTP API, not one payload — component-schema selection is conventional, its dialect differs from draft 4 (`nullable`, `readOnly`, no type arrays), and no standard "validate this payload against this spec" operation exists. Allowing it would create an unenforceable tier behind the same conformance promise.

Validation uses the `jsonschema` crate with draft-4 support. The transitional carve-out for legacy `OpenApi3` registry content was dropped: the pre-implementation audit of every `acme-*` registry (task 5.3) found zero `OpenApi3` schemas — exactly one schema exists (`acme-billing@OrderNotification`, `JSONSchemaDraft4`) — so any unvalidatable stored content is simply the 503 `SchemaUnusable` path.

### 5. Message attributes carry admitted metadata

The validator sets SQS message attributes: `Environment` (from `$context.stage`, replacing the VTL injection), `SchemaVersion` (the admission-resolved version the payload was validated against), and the effective idempotency key (caller-supplied or the API Gateway request ID), plus the trusted-principal envelope the VTL used to write. The consumer derives `source` itself from the namespace resolver and builds the envelope from attributes plus the SQS `SentTimestamp` (unchanged `publishedAt` semantics). Envelope structural/size validation runs at admission (a publish that could never be delivered must not earn a 202) and stays in the consumer as a cheap local invariant re-check.

## Risks / Trade-offs

- [Ingest availability now depends on Lambda] → Explicit durability-boundary spec language (publisher owns the event until `202`); reserved concurrency and an admission-failure alarm on the validator.
- [TTL skew after a version append: validator may hold the old schema for up to 60s] → Refresh-before-reject makes false rejections impossible; stale *acceptance* of the old shape is benign under the additive-only norm (old shape validates against an additive successor anyway once refreshed).
- [Two Lambda invocations per publish instead of one] → Negligible cost at plausible volumes; revisit only if publish volume makes it material.
- [A validator bug blocks ingest entirely, where today's consumer bugs merely delay delivery] → Admission logic is pure and heavily unit-tested; deployment via the standard stack pipeline with rollback; `503` responses let well-behaved publishers retry.
- [Existing `OpenApi3` schemas would silently lose validation parity] → Pre-deploy audit of all registries for `OpenApi3` content; carve-out documented as transitional.

## Migration Plan

1. Deploy the validator Lambda and slimmed consumer behind the existing stages; the API Gateway integration swap (SQS → Lambda → SQS) is atomic per deployment.
2. Audit registries for `OpenApi3` content before enabling the create-time rejection (expected: none).
3. Rollback: redeploy the previous template; the direct SQS integration and consumer-side resolution return together. No data migration — the event-streams store and registries are untouched.

## Open Questions

None blocking. (Whether admission lives as a new `publish-admission` function or a rework of the API integration module is an implementation detail for tasks.md.)
