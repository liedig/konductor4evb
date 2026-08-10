## Context

See proposal.md - Why. Registration today is a conditional put on the domains table (`attribute_not_exists(domainName) OR status = REJECTED`) followed by the approval workflow start and outcome emission. DynamoDB offers no COUNT, and a scan-count at registration time would be both expensive and racy. The deployment serves a single organization, so per-deployment and per-organization are the same ceiling today; multi-organization hosting would revisit the scoping.

## Goals / Non-Goals

**Goals:**

- Deterministic ceiling enforcement at registration with no check-then-act race.
- Minimal change to the registration path and its existing idempotent-retry behavior.
- An operational story for existing deployments (counter seeding) and for capacity (alarm, quota validation).

**Non-Goals:**

- Domain deletion or slot reclamation (rejected domains consume slots until deletion exists).
- Per-team or per-account quotas beneath the platform ceiling.
- Dynamically adjustable caps or multi-account platform sharding.

## Decisions

### A counter item in the domains table, mutated in the registration transaction

A single counter item (distinct key outside the domain-name keyspace) is incremented by one in the same `TransactWriteItems` that creates a new domain row, with a condition that the count stays below the ceiling alongside the existing not-exists-or-rejected condition. Re-registration of a rejected domain overwrites its row without touching the counter. Concurrent registrations serialize on the counter item's condition, so exactly one transaction can commit the final slot. Rejected rows consume slots until domain deletion exists; that is acceptable because the cap is a blast-radius bound, not a billing meter.

Alternatives rejected: scan-count at registration (racy without a transaction and expensive at any table size), a separate counter table (same single-item contention with extra operational surface), and a handler-side check before the write (a classic check-then-act race under concurrency).

### The failure uses the established deterministic path

A cap breach maps to `409 Conflict` with a limit-exceeded reason and a `RegisterDomainFailed` outcome event carrying that reason, consistent with validation and duplicate-name failures: no workflow start, no retry (retrying cannot change the outcome), and the SQS message acknowledged.

### Operations treat the cap as a capacity boundary

Existing deployments seed the counter to the current domain count as a one-time migration step. A capacity alarm fires at 80% of the cap, and deployment validation confirms EventBridge quota headroom (custom buses and schema registries per account) covers the full cap, since the per-domain topology multiplies both resources in the platform account.

## Risks / Trade-offs

- **[Counter item is a single-point write contention]** → One atomic increment per registration; registration is low-TPS control-plane traffic, so contention is theoretical.
- **[Counter drifts from out-of-band table edits]** → The counter is a guard, not a ledger; a reconciliation runbook (count rows versus counter) covers drift, and seeding handles pre-existing deployments.
- **[Rejected domains permanently consume slots]** → Accepted and documented; domain deletion, when it arrives, decrements.

## Migration Plan

1. Deploy the counter-aware write path with seeding support.
2. Seed the counter to the current domain count on existing deployments (one-time task).
3. Enable the capacity alarm and record validated quota headroom in deployment docs.

Rollback removes the ceiling condition; the counter item is inert without it.

## Open Questions

None.
