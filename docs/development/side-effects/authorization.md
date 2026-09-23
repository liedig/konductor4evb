# Authorization write paths

[Documentation home](../../README.md)

Dated records from the [side-effects audit](README.md). These are rationale and failure analysis, not proof that the present code still has this shape.

## `entitlement-admin`

- **Committed writes:** one `TransactWriteItems` per route—conditional entitlement put + audit record (create), or conditional delete + audit record (revoke).
- **Side effects:** none in the handler; revocation fan-out rides the entitlements-table stream (the rubric's first application).
- **Decision:** STAY. **Rationale:** already satisfies the principle—the commit is the only synchronous step, and the stream carries the rest.

## `binding-admin`

- **Committed writes:** one `TransactWriteItems` per route—conditional binding put/delete + audit record.
- **Side effects:** none; bindings gate nothing materialized, so no revocation machinery exists to migrate (that arrives with any future binding-gated materialization).
- **Decision:** STAY. **Rationale:** one atomic commit, zero side effects.

## `entitlement-stream-consumer`

- **Committed writes:** conditional marker-item put (idempotency claim on the stream event id, 7-day TTL); best-effort marker delete on forward failure.
- **Side effects:** SQS revocation message + `EntitlementRevoked` business event, both built entirely from the stream record's old image.
- **Failure walk:** crash after claim before both sends → retry skips (marker held); the hourly sweep converges the missed disablement, but a crash between the SQS send and the event emission loses `EntitlementRevoked` permanently (the audit record committed with the delete remains the durable history). Accepted in the original design ("a lost revocation is worse than a duplicate").
- **Decision:** reference implementation—exempt from judgment per this audit's charter. Recorded for completeness.

## `entitlement-revocation-worker`

- **Committed writes:** per candidate, per enabled environment: `DisableRule` then `UpdateItem` (`enabled = false`); no transaction across the pair.
- **Side effects:** the rule disablement itself; no events.
- **Rubric:** (6) yes—the revocation SQS queue is the durable envelope; coverage is re-evaluated at consume time, so redelivery after a new grant legitimately cancels the disable.
- **Failure walk:** crash between disable and update → desired state says enabled while the rule is disabled; redrive/sweep converges. Covered.
- **Decision:** ENVELOPE-DRIVEN. **Rationale:** queue durability + idempotent-by-value writes + sweep backstop is the established command pattern.

## `entitlement-coverage-sweep`

- **Committed writes:** same disable path as the worker, driven by a full-table scan.
- **Side effects:** none beyond disablement.
- **Decision:** STAY. **Rationale:** the sweep *is* the reconciliation backstop other decisions rely on; there is no upstream envelope or transition to hang its work on.
