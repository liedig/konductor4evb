## 1. Storage and Models

- [x] 1.1 Add the domain-count counter item conventions to the domains-table access helpers in `konductor-common` (key construction, conditional increment), with unit tests
- [x] 1.2 Add the limit-exceeded outcome to the registration failure contract and the shared error models: a `DomainLimitExceeded` variant mapped to `409 Conflict` for synchronous surfaces, and the limit reason documented on the async registration path

## 2. Enforcement

- [x] 2.1 Move registration's two row-creating conditional puts — the `PENDING_APPROVAL` registration put and the collision-preflight `REJECTED` put — to `TransactWriteItems`, each creating the domain row and incrementing the counter atomically, conditioned below the cap; re-registration of a rejected domain overwrites the row without touching the counter, preserving the existing idempotent-retry behavior
- [x] 2.2 Map the cap condition failure to the deterministic failure path: `RegisterDomainFailed` with the limit reason, no approval workflow start, no retry (message acknowledged)
- [x] 2.3 Add behavior tests for at-cap rejection, the concurrent-registration race (exactly one of two wins the final slot), rejected-domain re-registration bypassing the cap, the collision-preflight put incrementing the counter on a new rejected row but not on overwrite of an existing one, and the counter remaining untouched on validation failure

## 3. Operations

- [x] 3.1 Add the one-time counter seeding path for existing deployments and a reconciliation runbook (count rows versus counter)
- [x] 3.2 Add a capacity alarm at 80% of the cap and validate EventBridge quota headroom (custom buses, schema registries per account) against the 1,000-domain cap, recording confirmed limits in the deployment docs

## 4. Documentation and Validation

- [x] 4.1 Update `docs/development/decisions.md` with the ceiling, the counter-based enforcement, and the rejected-slot trade-off
- [x] 4.2 Update `docs/development/architecture.md` with the domain-count ceiling and the counter item in the domains table
- [x] 4.3 Run `make check` and `make test` for the workspace
- [x] 4.4 Run strict OpenSpec validation and verify the implementation satisfies every domain-management scenario
