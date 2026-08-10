## Why

Control-plane handlers perform multi-step writes — a DynamoDB commit followed by Step Functions starts, EventBridge emissions, or queue sends — where a failure between steps leaves partial state, and every handler hand-rolls its own idempotent recovery to compensate. `add-subscriber-entitlements` introduces the project's first stream-driven side effect (revocation notification carried atomically by the entitlements table stream, translated into queue messages and business events). Before that pattern proliferates — or is ignored — inconsistently, every existing database-write path needs to be examined against the same principle, and each handler needs a recorded, evidence-based decision.

## What Changes

- Inventory every handler that commits a DynamoDB write and performs additional side effects in the same code path (domain registration and update, approval decision handling, subscription command outcomes, notification triggers, schema administration, and any others found).
- Classify each call site against the stream-driven side-effects constraints from the `add-subscriber-entitlements` design: whether the item carries everything a consumer needs, whether the side effect has a committed state transition to hang on, idempotency on stream event ids, tolerance for stream latency, 24-hour retention and shard-blocking operational needs, and whether an inbound command envelope already provides durability.
- Classify each emitted artifact as a change record (mechanical reaction, stays internal) or a business event (domain fact worthy of the control-plane bus, business-level audit, and future broader sharing).
- Record a per-handler decision — migrate to stream-driven, remain handler-driven with stated reasons, or already envelope-driven — in `docs/development/decisions.md`, spawning migration follow-ups as separate changes where approved.
- No production behavior changes: this change produces the audit, the classification, and the decision record only.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This change is an audit and decision record with no spec-level behavior changes; it opts out of spec deltas via `skip_specs: true` in `.openspec.yaml`. Migrations decided from the audit will declare capabilities in their own changes.

## Impact

- Reads all control-plane and event-streams handler code paths that write to DynamoDB; writes only `docs/development/decisions.md`.
- Depends on the stream-driven side-effects principle and constraints established in `add-subscriber-entitlements` (its design.md is the classification rubric).
- Expected follow-ups (decided, not executed, here): the `register.rs` three-system write (DynamoDB + Step Functions + EventBridge), the `update.rs` lifecycle event emission, and subscription outcome-event emission are the known migration candidates.
