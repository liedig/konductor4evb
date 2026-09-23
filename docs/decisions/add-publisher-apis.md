# Publisher APIs: environment-named stages, store-and-forward publish, append-only schema management—first grant-engine consumers

[Documentation home](../README.md)

- Recorded date: 2026-08-05
- Change ID: `add-publisher-apis`
- Area: Event Streams
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

**Environment-named stages**: both API surfaces replaced the static `v1` stage with `dev`/`staging`/`prod` stages sharing one deployment; the stage is the environment selector, so publishers never name an environment, registry, or bus.

**Flat verb endpoints**: `POST /publish`, `POST /schemas`, `POST /schemas/versions` take the domain in the body—no nested resource paths, matching the binding-admin precedent.

**Store-and-forward publish**: API Gateway validates the flat `{domain, eventName, data, idempotencyKey?}` body and queues it with the trusted-principal envelope, `Environment` (from `$context.stage`), and `IdempotencyKey` (caller key or `$context.requestId`) as message attributes; the `publish-events` consumer authorizes `schema:publish` via `DynamoGrantEngine` before any resource access, requires an ACTIVE domain, resolves the latest schema version in the stage-selected registry, builds the canonical envelope, and calls `PutEvents` on the domain bus. Deterministic denials are acknowledged; everything else retries via partial batch responses (batch 10) with immutable attributes.

**Envelope metadata immutability**: `publishedAt` derives from the SQS `SentTimestamp` (retries keep the original publish time) and `idempotencyKey` from the message attribute; the consumer validates the generated envelope (positive version, non-empty identity, serializable, ≤256 KB) before delivery—invalid generated envelopes are processing failures, never partial deliveries.

**Schema management mimics the EventBridge Schemas API**: synchronous proxy endpoints (the created version must return to the caller) mapping `CreateSchema` → 201 version 1 / 409 duplicate and `UpdateSchema` → 201 next version / 404 missing; append-only—no delete, retire, replace, or version deletion, and no registry administration permissions.

**Auth engine separation**: this change consumes the `ResourceAuthorizer` decision interface only; binding persistence and administration stay with `add-grant-engine`, and both publisher functions read bindings through the reserved `konductor-bindings-table-read` managed policy.

**Event-name feasibility**: schema writes validate the event name against EventBridge rule-name constraints (`rule_name`) before any service call.

**Duplicate-create mapping**: the Schemas service model has no `ConflictException` for `CreateSchema`, so a duplicate's `BadRequestException` ("already exists") maps to 409 with a `code()` fallback for a future conflict code.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-08-05-add-publisher-apis/proposal.md). The recorded date above is not necessarily the archive or merge date.

## Current relevance

The original consumer-side publish checks were superseded by [validation at ingest](validate-at-ingest.md). Retain this record for the stage, API, and envelope choices.

[Decision index](README.md)
