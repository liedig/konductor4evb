## Why

The platform's scale target is corrected from "tens of thousands of domains" to a hard ceiling of 1,000 domains per deployment: realistic usage sits in the low hundreds, and the per-domain bus and registry topology makes an unbounded domain count physically unreachable in a single platform account (default quotas are 100 custom buses and roughly 100 schema registries per account). A ceiling without enforcement is a wish — registration must reject growth beyond the cap deterministically so quota planning, monitoring bounds, and blast radius stay honest.

## What Changes

- Enforce a domain-count ceiling of 1,000 at registration: creating a new domain beyond the cap fails with `409 Conflict` and a limit-exceeded reason; no domain row is created, no approval workflow starts, and the failure follows the established deterministic outcome-event path.
- Enforcement is atomic with domain creation: a counter item in the domains table is incremented in the same transaction as the domain row, conditioned below the cap, so concurrent registrations cannot race past it.
- Re-registering a rejected domain and updating existing domains never count against the cap; rejected domains — including system-rejected rows recorded by the collision preflight — consume slots until domain deletion exists (accepted trade-off).
- Add a one-time counter seeding path for existing deployments, an 80%-of-cap capacity alarm, and EventBridge quota-headroom validation against the cap.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `domain-management`: registration enforces the domain-count ceiling with a deterministic limit error.

## Impact

- The domain-management registration handler and its write path: the conditional put becomes a transactional write carrying the counter, and the registration API gains a limit-failure contract.
- Deployment operations: counter seeding for existing deployments, a capacity alarm, and quota validation (custom buses and schema registries per account) against the 1,000-domain cap.
- The scale-target context in `openspec/config.yaml` is corrected alongside this change; `docs/development/architecture.md` gains the ceiling and counter item.
