# Design: harden-workflow-failure-paths

## Context

Live verification on 2026-07-15 (platform account 444433332222, ap-southeast-2) proved that the domain-management workflows' failure paths crash inside their own error handling. Three `provision-domain` executions died at `EmitProvisioningFailedAfterRollback` with `States.QueryEvaluationError`; the approval workflow's rejection path dereferences a variable that is never assigned on that path. In both cases the workflow's terminal obligations — write a terminal domain status, emit exactly one lifecycle event — are silently skipped: domains stick in `PROVISIONING`/`PENDING_APPROVAL`, owners are never notified, and operators discover the hang by inspection.

The bugs share a root cause: JSONata expressions and `Assign`/`Catch` semantics that were only exercisable by real failures. Design review missed them; unit tests can't reach them (state machine ASL isn't covered by the Rust test suite); only live SES/quota failures surfaced them.

Constraints:

- ASL is YAML + JSONata (`QueryLanguage: JSONata` per repo convention); no Lambda shims for logic that JSONata can express.
- The decide Lambda's contract is already correct (`SendTaskFailure` with error `RegistrationRejected`, cause = JSON `{approved, decidedBy, justification}`) — the workflow must consume it, not change it.
- The domain status model (`domain-management` spec) allows only enumerated transitions; any new failure outcome must fit or extend that model explicitly.
- `nexus-core` is pre-production; no live executions need migration, but stuck records from past crashes exist in the dev deployment.

## Goals / Non-Goals

**Goals:**

- Every execution path of `approval` and `provision-domain` terminates with (a) a terminal domain-status write and (b) exactly one lifecycle event — under all failure shapes observed and constructible.
- Distinguish the three distinct `NotifyAndWait` outcomes that currently collapse into one catch: human rejection, notification-infrastructure failure, and timeout.
- Transient AWS API errors (throttling foremost) retry with backoff instead of failing an environment and triggering saga rollback.
- Failure-path behavior is verifiable against a deployed dev stack with forced failures.

**Non-Goals:**

- No changes to `activate-subscription` / `remove-subscription` (owned by `add-event-subscriptions`; a follow-up should mirror the retry posture there once this lands).
- No changes to Lambda code, API contract, table schemas, or the decide Lambda's failure encoding.
- No composite/alerting work on top of the emitted failure events (existing notification Lambdas already subscribe).
- No generic ASL testing framework — verification is scenario-driven against the dev stack.

## Decisions

### D1: Fix `compensationErrors` aggregation with an array constructor, not a guard

Replace (provision-domain.asl.yaml:269):

```
$exists($rollbackResults) ? $reduce($rollbackResults[$exists(compensationErrors)].compensationErrors, function($a,$b) { $append($a,$b) }, []) : []
```

with:

```
[$rollbackResults.compensationErrors]
```

JSONata's array constructor coerces undefined to `[]` and sequence-flattens nested arrays, handling all four shapes: rollback never ran (variable unassigned — see D2), rollback over zero envs, clean rollback (`compensationErrors: []` per env), rollback with errors. Alternative considered: keep the `$reduce` but guard with `$count($rollbackResults) > 0` — rejected because it still breaks on the clean-rollback shape (mapping `.compensationErrors` over all-empty arrays yields undefined) and is strictly more code for less coverage.

**Corollary**: `$rollbackResults` must be *assigned* on every path that reaches `EmitProvisioningFailedAfterRollback`. Today it is assigned only by the `RollbackSucceededEnvs` Map, which always runs before that state, so the constructor alone suffices — but initialize it to `[]` alongside the failure detection (defensive, zero cost) so future path reordering can't reintroduce the crash class.

### D2: Split the approval catch into three routes; parse rejection metadata from the error cause

`NotifyAndWait` gains three catches, evaluated in ASL order:

| Catch | Route | Semantics |
|---|---|---|
| `RegistrationRejected` (custom error from decide Lambda) | `ParseRejection` → `UpdateStatusRejected` | Human decision. `decidedBy`/`justification` recovered via `$parse($states.errorOutput.Cause)` |
| `States.Timeout` | `UpdateStatusRejected` (timeout variant) | 7-day expiry = rejection, honoring documented semantics (see D4). `decidedBy: 'system:timeout'`, justification fixed string |
| `States.ALL` (residual) | `UpdateStatusApprovalFailed` | Notification-infrastructure failure. Domain is NOT rejected (see D3) |

Rationale for order: custom error name is the most specific signal and the decide Lambda already emits it; `States.Timeout` is not matched by `States.TaskFailed` so it needs its own clause; the residual catch keeps the guarantee total. Alternative considered: have the decide Lambda call `SendTaskSuccess` with `approved: false` instead of `SendTaskFailure`, making rejection a normal continuation — cleaner in ASL (no cause parsing) but changes the Lambda contract and the `domain-management` spec's decide-registration requirement, and loses nothing we need. Rejected to keep the blast radius inside the workflows; noted as a possible future simplification.

### D3: `$registration` captured by a Pass state before `NotifyAndWait`

Insert `CaptureRegistration` (Pass, `Assign: registration: "{% $states.input %}"`) as the new `StartAt`. `Assign` on a Task applies only on success — that is the root cause of the rejection-path crash — so the capture must happen in a state that cannot fail. Alternative considered: duplicating `Assign` into each Catch's `Output` — rejected: catch outputs receive the error object, not the original input; the registration context is simply not available there.

### D4: Notification failure gets its own status: `APPROVAL_NOTIFICATION_FAILED`

When the notification Lambda fails (SES unverified, misconfiguration), no human decided anything: marking the domain `REJECTED` is dishonest and terminal, and leaving `PENDING_APPROVAL` is a lie — no approval email is pending. Add one status to the domain status model:

- `APPROVAL_NOTIFICATION_FAILED` — "registration accepted but the approval request could not be delivered; operator intervention needed" (whose turn: Operator).
- Permitted transitions: `PENDING_APPROVAL` → `APPROVAL_NOTIFICATION_FAILED` (approval workflow, notification-failure path). Recovery is operator-driven re-registration or a future re-notify action; no automated transition out is defined in this change.
- Emits `DomainApprovalNotificationFailed` (new event, control-plane bus) carrying `domainName`, `errorMessage` — the platform operator is the audience; the existing owner-facing notification Lambdas do not subscribe to it (an owner email would be absurd: email delivery is exactly what failed).

Alternatives considered: (a) reuse `PROVISIONING_FAILED` — wrong phase, misleading for operators filtering by status; (b) retry the notification Lambda and treat exhaustion as rejection — retries added (D5) but exhaustion still isn't a human decision; (c) no status change, event only — leaves the record stating an approval is pending when none is. Timeout, by contrast, keeps its documented rejection semantics: a 7-day silence is a decision-by-default and the README already promises it.

### D5: Retry posture — one profile per API class, applied to every SDK task state

All SDK task states in the four state machines (`approval`, `provision-domain`, `provision-env`, `deprovision-env`) get `Retry` blocks ahead of their `Catch`:

| Profile | Applies to | Policy |
|---|---|---|
| Throttle-prone control plane | CloudWatch `putMetricAlarm`/`putDashboard`/`deleteAlarms`/`deleteDashboards`, Schemas `createRegistry`/`deleteRegistry` | `IntervalSeconds: 2, MaxAttempts: 5, BackoffRate: 2.0, JitterStrategy: FULL` on throttle/5xx error names |
| Standard control plane | EventBridge, SQS, SSM, RAM, DynamoDB, `events:putEvents`, `startExecution` | `IntervalSeconds: 1, MaxAttempts: 3, BackoffRate: 2.0, JitterStrategy: FULL` |

Retried error names: the service throttle exceptions (`Schemas.TooManyRequestsException` is ambiguous — see Risks), `ThrottlingException`, `TooManyRequestsException`, `ServiceUnavailable`, `InternalFailure`, and `States.TaskFailed` is deliberately NOT retried wholesale — business errors (`ResourceAlreadyExistsException`, `ConditionalCheckFailed`) must fail fast into compensation. Compensation/teardown steps in `deprovision-env` and `provision-env`'s inner rollback chain get the same profiles: a throttled delete currently records a false compensation error and, worse, leaks the resource.

`NotifyAndWait` itself gets a Lambda-invoke retry (`Lambda.ServiceException`, `Lambda.TooManyRequestsException`, `Lambda.AWSLambdaException`; not the task-token wait) so transient invoke failures don't hit the D4 path; a *successful invoke whose handler fails* (SES error surfaced via `SendTaskFailure`? No — the notification Lambda raises, producing `States.TaskFailed` with the Lambda error name) still routes to D4 after retries.

### D6: Verification is forced-failure scenarios against the dev stack, not unit tests

ASL logic has no local test harness in this repo. Each historical failure shape is reproduced deliberately: (1) zero-env success — temporarily point `provision-env` at an env with the schemas quota exhausted (or a artificially low applied quota); (2) partial failure with clean rollback — poison one env's input; (3) rejection via the real decide API; (4) notification failure — deploy with an unverified `ApprovalFromEmail`; (5) timeout — not feasible at 7 days; validated by ASL review plus a temporary `TimeoutSeconds` override in a dev-only deploy. Each scenario asserts: terminal DDB status, exactly one lifecycle event on `nexus-bus` (observed via a temporary catch-all rule → CloudWatch Logs), and zero `QueryEvaluationError` in the execution history.

## Risks / Trade-offs

- **[Retrying `Schemas.TooManyRequestsException` retries a hard quota error]** The schemas service uses one error code for both rate throttling and the registry count limit; retrying the count limit wastes ~1 minute before failing. → Accept: 5 bounded attempts add ~62s worst case to an already-failing provisioning; correctness (transient throttles survive) outweighs latency of a rare hard failure. The quota doc already tells operators to raise the limit preemptively.
- **[New status value ripples into consumers]** `APPROVAL_NOTIFICATION_FAILED` must be accepted by the Rust `DomainStatus` enum, list/describe filters, and any dashboards keying on status. → Mitigation: grep-driven checklist in tasks; serde `other` fallback already tolerates unknown statuses on read paths (verify in tasks; if absent, add the variant everywhere).
- **[Cause parsing couples workflow to decide-Lambda encoding]** `$parse($states.errorOutput.Cause)` assumes the cause is the JSON the decide Lambda writes. A malformed cause (manual `SendTaskFailure` via CLI) would crash the parse. → Mitigation: guard the parse (`$type()` check) and fall back to `decidedBy: 'unknown'` — the rejection still lands; the metadata degrades gracefully.
- **[Retry blocks lengthen genuine-failure latency]** All-or-nothing rollback now waits for retries to exhaust per env. → Accept: minutes, not hours; the alternative (fast rollback of provisioning that would have succeeded on retry) costs an operator round-trip.
- **[Timeout scenario not fully testable]** 7-day timeout validated only via short-override in dev. → Accept; the catch clause is two lines and the override test exercises the identical path.

## Migration Plan

1. Deploy updated `nexus-core` stack (state-machine definitions only; no resource replacement).
2. One-time cleanup: list domains stuck in `PROVISIONING` (or `PENDING_APPROVAL` with a dead task token) from historical crashes; delete or re-register in dev. Document the query in the runbook section of `docs/domain-management.md` troubleshooting.
3. Update `docs/service-quotas.md`: remove the "no Retry blocks / approve one at a time" caveat, keep the PutMetricAlarm quota note as advisory.
4. Rollback: redeploy prior template — state machine definition changes are versionless and instant; no data migration in either direction.

## Open Questions

- Should the decide Lambda's rejection eventually move to `SendTaskSuccess` + `approved: false` (D2 alternative), retiring cause-parsing? Deferred — would simplify ASL but touches the Lambda contract and spec; candidate for a later cleanup change.
- Does anything need to *auto-recover* from `APPROVAL_NOTIFICATION_FAILED` (e.g., re-drive after SES verification)? Deferred until the notification-transport story (decisions.md: extract-notifications-stack "alternate transports") matures.
