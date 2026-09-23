# Fixed JSONata crashes in workflow failure paths; added APPROVAL_NOTIFICATION_FAILED status; added Retry policies to all SDK task states

[Documentation home](../README.md)

- Recorded date: 2026-07-16
- Change ID: `harden-workflow-failure-paths`
- Area: Domain Management
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

**Root cause**: the failure paths of `approval.asl.yaml` and `provision-domain.asl.yaml` were never exercised until real failures (SES unverified, schema-registry quota exhaustion) hit them live on 2026-07-15, revealing that the error-handling itself crashed with `States.QueryEvaluationError`—silently leaving domains stuck in `PROVISIONING`/`PENDING_APPROVAL` with no notification and no terminal event.

**`provision-domain` fix**: replaced a `$exists`-guarded `$reduce` over rollback compensation errors (which returns undefined—and crashes the state—for the two most common shapes: zero envs succeeded, or rollback completed cleanly) with a JSONata array constructor (`[$rollbackResults.compensationErrors]`) that coerces undefined to `[]` and flattens for every shape.

**`approval` rewrite**: registration context is now captured in a Pass state (`CaptureRegistration`) *before* the task-token wait, since `Assign` on a Task only applies on success and was leaving `$registration` unassigned on every failure path. The single `States.TaskFailed` catch is split into three routes: the `RegistrationRejected` custom error (human rejection, with `decidedBy`/`justification` parsed from the decide Lambda's JSON cause, falling back to `unknown` on a malformed cause), `States.Timeout` (7-day silence, `decidedBy: system:timeout`), and a residual `States.ALL` route for notification-infrastructure failure—previously indistinguishable from a human rejection.

**New terminal status**: `APPROVAL_NOTIFICATION_FAILED`—a notification-delivery failure means nobody decided anything, so it is neither `REJECTED` (dishonest) nor `PENDING_APPROVAL` (no request is actually pending); emits a new operator-facing `DomainApprovalNotificationFailed` event with no owner-facing email (email delivery is the failed dependency).

**Retry posture**: added `Retry` blocks (throttle-prone profile: 2 s/5 attempts for CloudWatch/Schemas; standard profile: 1 s/3 attempts for EventBridge/SQS/SSM/RAM/DynamoDB/putEvents/startExecution) to every SDK task state across all four domain-management state machines, forward and compensation paths alike, excluding deterministic business errors so they still fail fast into saga rollback.

**Live-verified, not just unit-tested**: deployed to the platform account and confirmed against real executions—a registration coincidentally hit the platform's actual schema-registry quota mid-verification, reproducing the exact historical partial-failure crash shape; the outer workflow that previously died with `QueryEvaluationError` instead completed with `ExecutionSucceeded`, correctly rolled back the succeeded environments, and left the domain in `PROVISIONING_FAILED` with an accurate `compensationErrors: []`. The rejection, notification-failure, and happy-path-forward scenarios were each verified the same way via direct Step Functions executions and decide-API calls against the live stack.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-07-16-harden-workflow-failure-paths/proposal.md). The recorded date above is not necessarily the archive or merge date.

[Decision index](README.md)
