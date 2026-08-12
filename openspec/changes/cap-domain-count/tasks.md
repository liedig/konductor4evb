## 1. Storage and Models

- [ ] 1.1 Add the domain-count counter item conventions to the domains-table access helpers in `konductor-common` (key construction, conditional increment), with unit tests
- [ ] 1.2 Add the limit-exceeded mapping (`409 Conflict` with limit reason) to the registration API contract and error models

## 2. Enforcement

- [ ] 2.1 Move registration's conditional put to a `TransactWriteItems` that creates the domain row and increments the counter atomically, conditioned below the cap; re-registration of a rejected domain overwrites the row without touching the counter, preserving the existing idempotent-retry behavior
- [ ] 2.2 Map the cap condition failure to the deterministic failure path: `409 Conflict`, `RegisterDomainFailed` with the limit reason, no approval workflow start, no retry
- [ ] 2.3 Add behavior tests for at-cap rejection, the concurrent-registration race (exactly one of two wins the final slot), rejected-domain re-registration bypassing the cap, and the counter remaining untouched on validation failure

## 3. Operations

- [ ] 3.1 Add the one-time counter seeding path for existing deployments and a reconciliation runbook (count rows versus counter)
- [ ] 3.2 Add a capacity alarm at 80% of the cap and validate EventBridge quota headroom (custom buses, schema registries per account) against the 1,000-domain cap, recording confirmed limits in the deployment docs

## 4. Documentation and Validation

- [ ] 4.1 Update `docs/development/decisions.md` with the ceiling, the counter-based enforcement, and the rejected-slot trade-off
- [ ] 4.2 Update `docs/development/architecture.md` with the domain-count ceiling and the counter item in the domains table
- [ ] 4.3 Run `make check` and `make test` for the workspace
- [ ] 4.4 Run strict OpenSpec validation and verify the implementation satisfies every domain-management scenario
