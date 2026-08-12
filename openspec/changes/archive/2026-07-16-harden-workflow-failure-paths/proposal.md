# Proposal: harden-workflow-failure-paths

## Why

The domain-management workflows handle failures with saga compensation and terminal status updates — but the failure paths themselves contain JSONata evaluation bugs that crash the workflow *inside its error handling*. The result inverts the platform's reliability contract: instead of "provisioning failed, owner notified, record marked `PROVISIONING_FAILED`", the observed behavior is "workflow dies with `States.QueryEvaluationError`, no event emitted, no notification sent, record stuck in `PROVISIONING` forever". These bugs were invisible until real failures (SES unverified, schema-registry quota exhaustion) exercised the paths during live verification on 2026-07-15.

Confirmed production evidence (platform account 444433332222, ap-southeast-2):

1. **`provision-domain` crashes on every realistic failure.** Three executions (`1096a37e-…` at 21:21, `1ed3b0bf-…` and `02e6c040-…` at 16:40) all died at `EmitProvisioningFailedAfterRollback` with `States.QueryEvaluationError`. The `compensationErrors` expression (`provision-domain.asl.yaml:269`) guards with `$exists($rollbackResults)`, but `$exists([])` is true in JSONata, and mapping `.compensationErrors` over empty (or all-empty) rollback results collapses to undefined, which `$reduce` propagates. The expression only survives when rollback *itself* had errors — the two most common failure shapes (no env succeeded; rollback succeeded cleanly) both crash. Neither `UpdateDomainProvisioningFailed` nor the `DomainProvisioningFailed` event is ever reached.

2. **`approval` workflow's rejection/failure catch path is broken three ways** (`approval.asl.yaml:20-27`):
   - `$registration` is assigned via `Assign` on the `NotifyAndWait` task, but `Assign` only applies on task *success*. On any `States.TaskFailed` — including a legitimate rejection delivered via `SendTaskFailure` — the catch routes to `UpdateStatusRejected`, which dereferences `$registration.domainName` → `QueryEvaluationError`.
   - On the catch path, `$states.input` is the error output (`{Error, Cause}`), not the decide payload — so `$states.input.decidedBy` / `.justification` are undefined (the decide Lambda packs them into the `Cause` JSON string, which is never parsed).
   - A notification-delivery failure (e.g., SES identity unverified — observed live) raises the same `States.TaskFailed` as a human rejection, so an email infrastructure error would mark a domain `REJECTED` even though nobody decided anything. Additionally, the 7-day `TimeoutSeconds` raises `States.Timeout`, which the `States.TaskFailed` catch does NOT match — a timed-out approval fails the execution with no status update and no event, despite the README documenting timeout-as-rejection.

3. **No retry posture on workflow SDK calls.** None of the four domain-management state machines (`approval`, `provision-domain`, `provision-env`, `deprovision-env`) declare `Retry` blocks. Throttling — CloudWatch `PutMetricAlarm` defaults to 3 TPS while each provisioning fires 6 parallel calls; EventBridge control plane is 50 TPS shared — fails an environment outright, triggering full saga rollback for a transient condition. This is precisely the traffic that drives executions down the broken failure paths above (see `docs/service-quotas.md`, "Control-plane throttling during provisioning").

Why now: the failure paths are provably unexercisable by design review alone — they were only caught by live failures. With `add-event-subscriptions` deployed and quota-driven failures documented as an expected operational condition, every future onboarding wave will route real traffic through these paths.

## What Changes

- **Fix `provision-domain` failure aggregation**: replace the `$exists`-guarded `$reduce` with an expression that yields `[]` for absent/empty/clean rollback results (JSONata array constructor `[$rollbackResults.compensationErrors]` coerces undefined and flattens). The outer workflow reaches `UpdateDomainProvisioningFailed` and emits `DomainProvisioningFailed` for **every** failure shape: zero envs succeeded, partial success with clean rollback, partial success with rollback errors.
- **Rework the `approval` workflow terminal paths**:
  - Capture registration context so it is available on failure paths (assign before/independent of task outcome, e.g., a Pass state ahead of `NotifyAndWait`).
  - Split the single `States.TaskFailed` catch: a `RegistrationRejected` custom error (sent by the decide Lambda) routes to the rejection path with `decidedBy`/`justification` parsed from the error cause; other task failures (notification Lambda errors) route to a distinct system-failure path that does **not** mark the domain `REJECTED`.
  - Catch `States.Timeout` explicitly and honor the documented timeout-as-rejection semantics (or revise the documentation — decision in design).
  - Guarantee: every execution path ends with a domain-record terminal status write and exactly one lifecycle event.
- **Add `Retry` blocks to all SDK task states** in the four domain-management state machines, with backoff tuned per API class (CloudWatch alarm/dashboard writes are the tightest at 3–10 TPS). Compensation/teardown steps retry too — a throttled delete during rollback currently records a spurious compensation error.
- **Verification**: reproduce each historical failure shape against the dev stack (forced env failure with zero successes; forced notification failure; rejection via decide API) and assert terminal status + emitted event.

Out of scope: the event-streams workflows (`activate-subscription`, `remove-subscription`) — their compensation design is owned by the in-flight `add-event-subscriptions` change; a retry-posture follow-up there should ride on that change or its successor. No API contract, Lambda code, or table schema changes.

## Capabilities

### New Capabilities

None — this hardens existing behavior; no new surface area.

### Modified Capabilities

- `domain-provisioning-workflow`: failure handling SHALL be total — the outer workflow reaches its terminal failure states (status update + `DomainProvisioningFailed` with aggregated `compensationErrors`, empty array when none) for every combination of env failure and rollback outcome; all workflow SDK task states SHALL retry throttling/transient service errors before failing the step.
- `domain-management`: the approval workflow SHALL distinguish human rejection from notification-delivery failure and from timeout; rejection metadata (`decidedBy`, `justification`) SHALL be recovered from the `SendTaskFailure` cause; every approval-workflow execution path SHALL leave the domain record in a defined terminal state and emit exactly one lifecycle event.

## Impact

- **State machines**: `domain-management/state-machines/approval.asl.yaml`, `provision-domain.asl.yaml`, `provision-env.asl.yaml`, `deprovision-env.asl.yaml` (JSONata fixes, catch restructuring, Retry blocks). No new resources; `konductor-core` redeploy only.
- **Decide Lambda contract**: unchanged (it already sends error `RegistrationRejected` with structured cause); the workflow finally consumes what it sends.
- **Docs**: `docs/development/architecture.md` (approval failure-path semantics), `docs/service-quotas.md` (remove the "no Retry blocks / approve one at a time" operational caveat once retries land), README workflow section if timeout semantics change.
- **Operational**: stuck-in-`PROVISIONING` records caused by past crashes need one-time manual cleanup (delete or re-approve); noted for the runbook.
- **Risk**: low — changes are confined to failure paths and retry policy; happy paths are untouched. Main design tension is the outcome status for notification-delivery failure (no existing status fits; options weighed in design.md).
