## Why

The Cognito access foundation wired every consumer behind the `ResourceAuthorizer` trait, but the only implementations are an unconditional deny-all and an interim IAM-only bridge. The `evaluate-authorization-engine` spike validated a DynamoDB-backed grant model as conditionally ready (13/13 scenarios, 34/34 steps, mandatory correctness and non-disclosure proven in-cloud) and explicitly directed that a separate change build the production architecture. This change implements the production grant engine: binding storage, decision evaluation, administration, and audit, so that `add-publisher-apis` can enforce publisher authorization and `add-subscriber-entitlements` can build on the same engine later.

## What Changes

- Add a dedicated bindings table (`RoleBinding` + `AuditRecord` item types, admin GSI) holding publisher and subscriber role bindings keyed by typed principal (issuer-qualified Cognito team group or twelve-digit AWS account) and domain.
- Add `DynamoGrantEngine` (production, strongly consistent decision reads) and `InMemoryGrantEngine` (fixture-fed twin for tests and local development), both implementing the existing `ResourceAuthorizer` trait with string action/resource parameters and `Allow`/`Deny` outcomes.
- Add flat-verb binding administration APIs on the OAuth surface only, gated to `platform-admin`: `POST /bindings`, `GET /bindings?domain=…`, `DELETE /bindings/{bindingId}`. Binding mutations use conditional writes and write token-free audit records atomically with each mutation.
- Rename existing authorizers to match their function: `FailClosedAuthorizer` → `DenyAllAuthorizer`, `FoundationAuthorizer` → `IamOnlyAuthorizer`.
- Wire no existing consumer to the engine: publish/schema consumers arrive with `add-publisher-apis`, and subscribe paths stay on `IamOnlyAuthorizer` until `add-subscriber-entitlements` adds entitlements. The engine ships dark with the binding administration surface live.
- Add a development seed helper (script or `make` target) that creates bindings for test accounts and groups so local development and testing stay straightforward.
- Confirm the spike's conditional latency SLOs with an in-region measurement (residual risk #1) and apply conditional writes for concurrent grant mutation (residual #2) with semantic audit items (residual #5).
- Defer the entitlements table, entitlement administration, revocation worker, and subscribe-path enforcement to `add-subscriber-entitlements`. Defer the durable command-idempotency store to the first change with a send-once side effect. Defer `schema:discover` fan-out capping to the subscriber change.

## Capabilities

### New Capabilities

- `grant-engine`: Production binding storage, additive grant evaluation with default denial, strongly consistent decision reads, conditional mutation writes, atomic token-free audit, platform-admin binding administration, and in-memory evaluator parity for local testing.

### Modified Capabilities

- `control-plane-access`: Add `platform-admin`-gated binding administration routes to the OAuth surface; the IAM surface has no binding administration.

## Impact

- Adds one DynamoDB table with an admin GSI to the core stack, with least-privilege read access for future publisher consumers and admin write access for the binding administration Lambda.
- Adds a binding administration Lambda and three OAuth routes (`POST /bindings`, `GET /bindings`, `DELETE /bindings/{bindingId}`); no IAM-surface routes are added.
- Renames two types in `nexus-common` (`DenyAllAuthorizer`, `IamOnlyAuthorizer`) and updates consumer wiring to the new names; behavior is unchanged.
- `add-publisher-apis` loses its authorization prerequisite: the publish consumer and schema handlers can wire to `DynamoGrantEngine` directly, and its tasks for a production authorization adapter are satisfied by this change.
- No existing API behavior changes: domain-management and subscription paths continue on `IamOnlyAuthorizer` (IAM allow, OAuth deny).
