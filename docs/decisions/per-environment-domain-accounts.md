# Per-environment account overrides for domain provisioning

[Documentation home](../README.md)

- Recorded date: 2026-09-24
- Change ID: `per-environment-domain-accounts`
- Area: Domain Management
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

**Account-per-domain stays the default; environments may be pinned elsewhere.** A registration may carry an optional, sparse `environmentAccounts` map that overrides the account individual environments are provisioned into; `ownerAccountId` remains the default for every environment. Keys must be members of the platform's configured environment list, values must be 12-digit account IDs, and duplicates are accepted — sharing one account across environments is a supported topology. The map is fixed at registration: the update API does not accept the field, so changing an environment's account remains a deprovision-and-reprovision flow, aligned with the existing per-environment resource lifecycle.

**Sparse on the record, resolved at emission, one shared rule.** The domain record stores the map exactly as submitted — no materialized effective map that could drift. Resolution (`environmentAccounts[env]` ?? `ownerAccountId`) lives in a single `konductor-common` helper used by the Rust emitters (register, describe, pending-registrations, domain-state retry); ASL definitions pass the effective map through opaquely rather than re-implementing the rule in JSONata. Events and approval surfaces carry the effective map so approvers and downstream consumers see every account that receives resources; the sparse record remains for intent and audit.

**Trust validation stays at provisioning time.** Registration never calls into mapped accounts; an unreachable trust role fails that environment's provisioning, whose saga compensation and `PROVISIONING_FAILED` path already report it. Request-time `AssumeRole` probes were rejected: new IAM permissions on the command path, added latency, and a partial-failure UX the platform deliberately avoids at request time.

**New-account detection is unchanged, with the approval email as the compensating control.** Detection stays defined on `ownerAccountId` via the `by-owner` GSI; an account used only through overrides never raises the new-account flag. Fan-out adjacency items indexing every mapped account were rejected as record-model rework for marginal gain — the approver sees every account in the effective map before any resource exists.

**Workflow inputs name the resolved account explicitly.** `provision-env` and `deprovision-env` take a resolved `accountId` (renamed from `ownerAccountId`, which silently meant "the one account"); the outer workflow computes it per environment and records it for rollback, which therefore never re-resolves. Cost-center retagging keeps per-environment iteration with per-environment credential resolution: every tag target is an environment-scoped resource, so a shared account is never processed twice for the same resource. In-flight executions started before the change are safe: an absent map in the payload falls back to the default account at every lookup site.

**Console presentation is minimal.** The request form gains one optional account field per platform-configured environment (empty means the owner account; sparse submission), and the approval card and domain detail render the effective environment → account map. Detail-page console links resolve against each environment's effective account so resources in override accounts stay linked.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-09-26-per-environment-domain-accounts/proposal.md) (archived 2026-09-26). The recorded date above is not necessarily the archive or merge date.

[Decision index](README.md)
