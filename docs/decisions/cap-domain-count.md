# Hard 1,000-domain ceiling enforced by a transaction-charged counter item

[Documentation home](../README.md)

- Recorded date: 2026-08-24
- Change ID: `cap-domain-count`
- Area: Domain Management
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

**Why a ceiling**: the domains table is unbounded while every domain provisions real quota-bound resources (registries, buses, alarms); a hard cap turns silent quota exhaustion into a deterministic, observable failure.

**Counter item, not ScanCount**: registration writes moved to `TransactWriteItems` that create the row and increment a counter item (`META#domain-count`) atomically, conditioned below the cap—exact under concurrency with O(1) cost, versus a scan that is both racy and increasingly expensive.

**Rejected rows consume slots**: collision-preflight REJECTED rows are real rows, so they charge the counter; re-registering a rejected name overwrites in place without a second charge. Rows are never deleted (no domain deletion yet), so the ceiling bounds *lifetime* registrations—pruning and counter decrement arrive with a future domain-deletion capability.

**Failure is async and deterministic**: at the cap, registration emits `RegisterDomainFailed` (limit reason) with no approval workflow and no retry; the `DomainLimitExceeded` → 409 mapping exists in the shared error model for any future synchronous registration surface.

**Observability**: an hourly `domain-count-reporter` Lambda publishes the counter via log metric filter (`Konductor/DomainManagement` `DomainCount`); an alarm fires at 800 (80%), and `docs/service-quotas.md` carries the seeding and reconciliation runbook plus quota headroom sized against the cap.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-08-24-cap-domain-count/proposal.md). The recorded date above is not necessarily the archive or merge date.

[Decision index](README.md)
