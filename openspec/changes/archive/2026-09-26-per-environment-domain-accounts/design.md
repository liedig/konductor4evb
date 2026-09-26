# Design

## Context

See proposal.md for motivation and scope. The constraints that shape the approach:

- The registration handler (`domain-registrations`) performs no account validation today; `ownerAccountId` is recorded and every account interaction happens later, in the Step Functions workflows, via the trust role `arn:aws:iam::<account>:role/{org}-konductor-trust-role`.
- The inner workflows (`provision-env`, `deprovision-env`) and the retag workflow build that ARN from an `ownerAccountId` input field (8 construction sites across the three ASL definitions).
- New-account detection queries the `by-owner` GSI with the default account only; the GSI stores one key per domain record.
- The approval workflow builds its notification and lifecycle events in ASL (JSONata) from its input, which the register handler constructs in Rust.
- The product is unreleased; no data migration is owed.

## Goals / Non-Goals

**Goals:**

- Accept and validate a sparse `environmentAccounts` map at registration; store it as submitted.
- Resolve `accountFor(env)` exactly once per emission path through a single shared rule, and pass the resolved account to every per-environment workflow step, including compensation.
- Present the effective environment-to-account map to the approver (email and console) before any resource is created.
- Keep single-account registrations byte-identical in behavior to today.

**Non-Goals:**

- No request-time trust-role reachability checks (D1).
- No `by-owner` GSI expansion to mapped accounts (D3).
- No account-change process for a provisioned environment (D4; future change).
- No per-account detection refinements, quota automation, or console polish beyond the minimal presentation (D5).

## Decisions

### D1: Trust validation stays at provisioning time

Registration never calls into mapped accounts. An unreachable trust role fails the affected environment's `provision-env` execution, whose existing saga compensation and `PROVISIONING_FAILED` path report it. *Alternative considered:* request-time `AssumeRole` probes from the register handler — rejected: new IAM permissions on the command path, added latency, and a partial-failure UX the platform deliberately avoids at request time (validation is asynchronous by design).

### D2: Sparse on the record, resolved at emission, one shared rule

- The domain record stores `environmentAccounts` exactly as submitted (sparse, optional) alongside the existing `ownerAccountId` default and the stamped `environments` list. No materialized effective map on the record — one source of truth, no dual-field drift.
- Resolution lives in a single `konductor-common` function (`resolve account = environmentAccounts[env] ?? ownerAccountId`), unit-tested, called by the two Rust emitters that construct payloads: the register handler (approval-workflow input and `RegisterDomainSucceeded` detail) and the domain-state handler (`DomainProvisioningRetryRequested` detail).
- ASL definitions never resolve anything; they pass the effective map through opaquely from input to events and per-environment invocations. This avoids a second, JSONata-encoded implementation of the rule.
- Events and the approval presentation carry the **effective** map (every environment resolved); the record's sparse map remains for intent and audit.

*Alternative considered:* stamp a materialized effective map onto the record at registration — rejected as denormalization that can drift from the sparse form and buys nothing once emitters share one function.

### D3: `by-owner` GSI and new-account detection unchanged

Detection stays defined on `ownerAccountId` (the default account); the approval notification gains the effective account map as the human review control. Documented limitation (now normative in the domain-provisioning-workflow delta): an account used only via overrides is not GSI-tracked and never raises the new-account flag. *Alternative considered:* fan-out adjacency items indexing every distinct mapped account — rejected: record-model rework and migration for marginal gain, since the approver sees every account in the map.

### D4: Mapping is immutable once registered

`PUT /domains/{domainName}` does not accept `environmentAccounts` (OpenAPI schema omits it; the update handler's known-field handling keeps it that way). Changing an environment's account means deprovisioning and re-provisioning that environment, deferred to a future change — deliberately aligned with the per-environment resource lifecycle rather than a new mutation path.

### D5: Console presentation is minimal

- The domain-request form gains an optional advanced section with one account field per platform-configured environment (empty means "default account").
- The approval card and the domain detail view render the effective map (env → account), sourced from the API fields added for the emitters (pending-registrations and describe-domain responses expose the map).

### D6: Workflow inputs name the resolved account explicitly

`provision-env` and `deprovision-env` take a resolved `accountId` input (renamed from `ownerAccountId`, which silently meant "the one account"); the outer workflow computes it per environment. `retag-domain` reads the record and resolves the trust-role credentials per environment. Cost-center retagging keeps per-environment iteration: every tag target is an environment-scoped resource from `resourceArns`, so a shared account is never tagged twice for the same resource and per-account grouping would add machinery without removing a call. Trust-role ARN construction sites are mechanical substitutions of the resolved account.

## Risks / Trade-offs

- **Approver overlooks the map and provisions into a wrong account** → the effective map is shown in the approval email and the console approval card before any resource exists; rollback/deprovision paths honor the same resolution, so a mistake unwinds cleanly.
- **Quota concentration in shared accounts** (the duplicate-accounts case) → update `docs/operations/service-quotas.md` guidance to count environments resolving into an account, not domains, when checking headroom.
- **Override-only accounts escape new-account detection** → accepted and documented (D3); the approval email is the compensating control; per-account detection can be added later without contract change.
- **Event consumers see a new field** → additive only; `environmentAccounts` appears alongside `ownerAccountId`, existing consumers unaffected.
- **Rollback of a deployment after records carry the new field** → the record model must tolerate the unknown attribute on read (verify no `deny_unknown_fields` on the domain record serde shape); the field is inert to older code.

## Migration Plan

Single stack deploy ships OpenAPI schemas, handlers, and ASL definitions together; the console ships after the APIs expose the map fields. Existing domain records (no `environmentAccounts`) resolve to the default account on every read path, so behavior is unchanged. Rollback is a stack revert; pre-existing records carry no new field, and records written in the window carry an inert attribute older code ignores (per the serde check above).

## Open Questions

None that change the specs, the approach, or the task breakdown. (Implementation detail settled during tasks: how the console learns the platform's environment list for the override form.)
