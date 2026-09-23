# Side effects and durability

[Documentation home](../../README.md)

Authoring rubric and dated write-path records. Use [AGENTS.md](../../../AGENTS.md#side-effects-and-durability) for the repository-wide requirement.

> **Snapshot, not a fresh audit:** The original audit is dated 2026-08-25, with named September additions and metadata records dated 2026-09-22. Preserve its failure walks and follow-up IDs. Before using a classification for changed code, verify it and record the new evidence. A `MIGRATE` row does not mean its follow-up shipped.

## Authoring rubric

> **Update (2026-09-16, `protect-domain-managed-resources`):** This change adds no handler write paths. It contains an organization-admin SCP template and UI presentation changes. No Lambda handler gained a DynamoDB write or synchronous side effect, so the audit classifications remain unchanged.

This audit covers every handler path that commits an Amazon DynamoDB write or performs a side effect. The rubric from `add-subscriber-entitlements` requires one synchronous commit. A table stream or an existing durable command envelope must drive other side effects.

Each record answers these questions:

1. Does the written item contain everything a stream consumer needs?
2. Does every side effect follow a committed state transition?
3. Can the side effect use a stream event ID for idempotency?
4. Can it tolerate seconds of stream latency?
5. Can a consumer meet the operational requirements for 24-hour retention and shard blocking?
6. Does an inbound command envelope already provide durability?

## Decision index

The index summarizes the classification and rationale for each write path.

| Call site | Decision | Follow-up |
|---|---|---|
| `register.rs`—registration path | MIGRATE | `stream-driven-domain-registration` |
| `register.rs`—collision-preflight path | MIGRATE | `stream-driven-domain-registration` |
| `update.rs` | MIGRATE | `stream-domain-lifecycle-events` |
| `update.rs` costCenter → retag workflow | STAY | Not applicable |
| `domain-state`—retry-provisioning | MIGRATE | `stream-domain-lifecycle-events` |
| `domain-state`—enable/disable | STAY | Not applicable |
| `finalize-approval` | STAY | Not applicable |
| `decide-registration` | STAY | Not applicable |
| `send-approval-request-notification` | STAY |—(risk noted) |
| Outcome/notification email handlers (5) | ENVELOPE-DRIVEN | Not applicable |
| `subscriptions/subscribe.rs` | MIGRATE | `stream-subscription-outcomes` |
| `subscriptions/update_state.rs` | STAY | Not applicable |
| `subscriptions/unsubscribe.rs` | STAY | `fix-unsubscribe-execution-idempotency` |
| `materialize-subscription-env` | ENVELOPE-DRIVEN | Not applicable |
| `subscription-extender` | ENVELOPE-DRIVEN | Not applicable |
| `publish-events` | ENVELOPE-DRIVEN | Not applicable |
| `publish-events-admission` | STAY | Not applicable |
| `schema-admin` | STAY | Not applicable |
| `entitlement-admin` | STAY | Not applicable |
| `binding-admin` | STAY | Not applicable |
| `entitlement-stream-consumer` | Reference (exempt) | Not applicable |
| `entitlement-revocation-worker` | ENVELOPE-DRIVEN | Not applicable |
| `entitlement-coverage-sweep` | STAY | Not applicable |
| `grant-benchmark` | STAY (non-production) | Not applicable |

## Records by topic

- [Domain management](domain-management.md)
- [Subscriptions](subscriptions.md)
- [Publishing and schemas](publishing.md)
- [Authorization](authorization.md)
- [Metadata](metadata.md)
- [Remaining inventory and rubric placement](inventory.md)

Keep new records with the affected topic. Record the date, change ID, committed writes, side effects, failure walk, classification, rationale, and any follow-up ID.
