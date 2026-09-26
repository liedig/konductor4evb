# Proposal

## Why

Domain teams commonly split their AWS accounts by environment — a shared non-production account plus an isolated production account is the usual minimum. Today every environment of a domain is provisioned into the single `ownerAccountId`, and the domain-management contract requires it ("All environments of a given domain SHALL be provisioned into the same `ownerAccountId`"). Teams that separate accounts per environment cannot express their topology, and all teams concentrate every environment's quota consumption in one account.

## What Changes

- Domain registration accepts an optional environment-to-account mapping. `ownerAccountId` remains the default account for every environment; a sparse `environmentAccounts` map overrides individual environments. Single-account domains are unchanged.
- Map values need not be unique: multiple environments MAY resolve to the same account (the non-production sharing case). Map keys SHALL be members of the platform's configured environment list; every environment resolves through default-plus-overrides. The registration API continues to not accept an environment list — the mapping selects accounts, never environments.
- Provisioning, cost-center retagging, and teardown resolve the target account per environment (`accountFor(env)`) instead of using `ownerAccountId` uniformly.
- The approval workflow presents the effective account map so the approver reviews which accounts receive resources.
- An environment's account binding is fixed once that environment is provisioned in this capability version. Changing an environment's account means deprovisioning and re-provisioning that environment; that process is deferred.
- Lifecycle events keep `ownerAccountId` (now the default account) and gain the effective mapping. Additive only; existing consumers are unaffected.
- The trust-stack story is unchanged per account: operators deploy it into each distinct account used by any approved domain.

**BREAKING** (contract amendment only): the domain-management requirement "All environments of a given domain SHALL be provisioned into the same `ownerAccountId`" is revised. No deployed API behavior breaks; single-account registrations remain valid and byte-identical.

## Capabilities

### New Capabilities

None. This extends the existing domain lifecycle and provisioning capabilities rather than introducing a new bounded context.

### Modified Capabilities

- `domain-management`: registration contract gains the optional account mapping; record shape and immutability rules revised (default account plus sparse overrides; per-environment bindings fixed once provisioned).
- `domain-provisioning-workflow`: per-environment steps resolve the target account from the mapping; compensation and retry semantics unchanged.
- `system-architecture`: the account-per-domain design principle is revised to account-per-domain-per-environment; resource naming is unaffected because domain resource names are environment-qualified.
- `resource-topology`: the per-environment resource placement contract moves from "all environments in `ownerAccountId`" to the account resolved per environment.

## Impact

Observed implementation touchpoints:

- `api/openapi-oauth.yaml` and `api/openapi-iam.yaml` registration request schemas, the domains table record shape, and `domain-management/functions/domain-registrations`.
- The `provision-env`, `retag-domain`, and teardown ASL definitions build trust-role ARNs from `{ownerAccountId}` today; each becomes a per-environment resolution.
- Lifecycle event payloads, approval notification content, and the console's domain request and detail surfaces.
- `by-owner` GSI semantics (default account only, or each mapped account) and per-account quota guidance: fan-in concentrates buses, rules, and DLQs in shared accounts, and any per-account workflow step must iterate distinct accounts, not map entries.
- Trust-stack runbook: one deployment per distinct account used, rather than one per domain.

This is an initial proposal-only capture, not an implementation-ready design. No API shape, record encoding, validation timing, or migration mechanism has been selected. No breaking change to deployed behavior is approved by this proposal.

### Decisions Deferred

Resolved 2026-09-24, before delta specs were written:

- Trust-role reachability is validated **at provisioning time, per environment** — matching today's behavior for `ownerAccountId`; a bad account fails that environment's provisioning through the existing saga/`PROVISIONING_FAILED` path. No request-time reachability check.
- Payload shapes: `environmentAccounts` stays **sparse on the request and the domain record**; lifecycle events and the approval presentation carry the **effective, fully resolved** environment-to-account map.
- `by-owner` indexing **stays on the default account** (`ownerAccountId`); new-account detection semantics are unchanged. Documented limitation: an account used only via overrides is not GSI-tracked and does not affect the new-account flag; the approval notification presents the effective map as the review control.
- An environment's account is **fixed once provisioned** in this capability version; `PUT` rejects map changes; the deprovision/re-provision process is deferred to a future change.
- Console presentation is minimal: the request form gains optional per-environment account overrides, and approval and detail views show the effective map.
