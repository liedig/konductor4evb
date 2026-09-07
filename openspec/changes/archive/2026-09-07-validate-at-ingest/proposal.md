# Proposal: Validate at Ingest

## Why

Publishing currently calls `DescribeSchema` on every publish (a per-request throttle ceiling) and never validates the payload against the schema document — the envelope's `schemaVersion` stamp is an unearned assertion, so version-pinned subscription rules route on an unverified claim. Because API Gateway integrates with SQS directly, a publisher gets `202 Accepted` before any check runs; rejections surface only as post-hoc DLQ entries, detached from the code that produced the event. The platform promises consumers "events conforming to the pinned schema" but cannot keep that promise.

## What Changes

- **Admission control moves to the edge**: a synchronous validator Lambda enters the publish path (API Gateway → validator → SQS). Before a message is queued, the validator performs grant-engine authorization, resolves the stage-selected environment's latest schema version, validates the payload against the schema document, and stamps the resolved version. Only authorized, conformant, deliverable events enter the durable queue.
- **Synchronous rejection**: invalid payloads and unauthorized publishes fail with a `4xx` response identifying the violation. The publisher owns the event until `202`; after `202` the platform owns delivery.
- **Schema cache**: the validator keeps an in-memory cache (latest version + content per `(environment, schema)`) with a short TTL, eliminating the per-publish `DescribeSchema` call. On validation failure only, the cache refreshes once and re-validates before rejecting, so a `400` always means "fails the actual latest," never "stale cache."
- **Publish consumer becomes delivery-only**: envelope construction and schema resolution move to the validator; resolved metadata (schema version, idempotency key) travels as SQS message attributes. The consumer reads attributes, builds the `PutEvents` entry, and retries transient delivery failures only — poison messages cease to exist as a category.
- **BREAKING — JSON Schema only**: schema creation (`POST /schemas`) accepts `schemaType: JSONSchemaDraft4` only. `OpenApi3` documents cannot be payload-validated unambiguously (component selection is by convention, dialect differs from draft 4), so they are rejected at create time rather than creating an unenforceable tier.
- Publish-time envelope validation (structure, canonical identity, EventBridge entry constraints) remains, now executed at admission.

## Capabilities

### New Capabilities

(none — this change reshapes existing capabilities)

### Modified Capabilities

- `event-publishing`: the store-and-forward publish path gains synchronous admission (authz + schema resolution + payload validation + version stamping) ahead of SQS; the publish consumer becomes delivery-only; rejection semantics move from post-hoc DLQ to synchronous `4xx`.
- `event-envelope`: envelope construction and metadata stamping move from the publish consumer to the admission step; the "publish-time validation is envelope-only" requirement is replaced by payload conformance validation against the schema document.
- `schema-management`: schema creation is constrained to `JSONSchemaDraft4` (breaking); the append-only version API is unchanged.

## Impact

- **Code**: new `publish-admission` Lambda (or reworked API Gateway integration) in `event-streams/functions`; `publish-events` consumer slimmed to delivery; `konductor-common` gains a schema-validation module (new `jsonschema` crate dependency); `schema-admin` create path rejects `OpenApi3`.
- **APIs**: `POST /publish` gains synchronous `400`/`403` responses (previously only `202`); `POST /schemas` rejects `OpenApi3` (breaking).
- **Infra**: API Gateway integration for `/publish` changes from direct SQS to Lambda → SQS on both OAuth and IAM surfaces (`konductor-core-stack.yaml`, `api/` OpenAPI specs); validator Lambda IAM, ConcurrencyConsiderations, and alarms.
- **Semantics**: the "accepted means never lost" durability boundary moves from API-call time to `202` time; pre-acceptance retry is the publisher's responsibility.
- **Dependencies**: adds `jsonschema` (draft-4 capable) to the workspace.
