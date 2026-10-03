# Proposal

## Why

A consumer domain team can only subscribe to producer schemas covered by an existing entitlement, and only producer publishers or platform admins can create entitlements. A team that needs a schema it cannot consume has no in-product way to ask: the request happens out-of-band (chat, email, meetings), leaving no record of the ask, no notification to the people who can grant it, and no audit link between a request and the entitlement that eventually resolves it.

## What Changes

- Introduce **schema access requests**: a consumer team requests coverage of a producer schema (exact name or scope pattern) with a justification, through the control plane and console.
- Requests are decided by **publisher principals on the producer domain** — the same delegated population that can create entitlements today. Platform admins retain their existing ability.
- **Approve** creates the corresponding entitlement atomically with the request's transition to approved; **deny** requires a justification (registration inbox precedent).
- Deliberately lightweight workflow: a request record in a dedicated table, SES notification to producer publishers on submission and to the requester on decision, and flat-verb control-plane routes. This is **not** the Step Functions task-token machinery used for domain registration — an approval resolves to a single conditional write, not a long-running orchestration.
- Pending requests expire after a bounded period; requesters can withdraw their own requests.
- Every transition commits a token-free audit record, matching entitlement audit conventions.

**Status: backlog capture.** This change is proposal-only on purpose — specs, design, and tasks are deferred until the capability is prioritized. Do not implement from this proposal without completing the remaining artifacts.

## Capabilities

### New Capabilities

- `schema-access-requests`: request lifecycle (submit, decide, withdraw, expire), notification of producer publishers on submission and requesters on decision, atomic entitlement creation on approval, and the audit trail for every transition.

### Modified Capabilities

- `web-ui`: consumer-facing request affordance and a producer-facing decision inbox (the inbox page pattern already exists for registration approvals).
- `subscriber-entitlements`: possibly — today the consumer schema catalog exposes only entitled schemas. Whether non-entitled schemas become requestable-visible (see open question) determines whether this spec changes.

## Impact

Eventual, when prioritized: domain-management service (new routes, dedicated table, decide handler), notifications (SES templates), web console (catalog affordance and producer inbox), grant engine (no changes expected — approval resolves to the existing entitlement create path).

No impact now: this capture adds planning artifacts only.

## Open Questions

- **Discovery model**: how requesters learn what exists to request — a metadata-only catalog of non-entitled schemas, producer opt-in discoverability, or request-by-exact-name only. Schema names can be sensitive (e.g. `EmployeeTerminated`), so this needs a deliberate decision before specs are written.
- **Request granularity**: exact schema names only, or scope patterns mirroring entitlement scope semantics.
- **Expiry window**: the bounded lifetime of a pending request, and whether expiry notifies.

## Tracking

- GitHub issue: https://github.com/liedig/konductor4evb/issues/44
