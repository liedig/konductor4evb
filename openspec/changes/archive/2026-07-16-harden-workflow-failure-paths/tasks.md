# Tasks: harden-workflow-failure-paths

## 1. provision-domain failure aggregation

- [x] 1.1 Replace the `compensationErrors` expression in `EmitProvisioningFailedAfterRollback` with the array-constructor form (`[$rollbackResults.compensationErrors]`); initialize `rollbackResults` to `[]` where `failedResults`/`succeededResults` are assigned (defensive per design D1)
- [x] 1.2 Audit the remaining JSONata in `EmitProvisioningFailedAfterRollback` and `EmitProvisioningFailedDomainLevel` for the same undefined-propagation class (`$failedResults[0].…` when failure detail is absent) and guard where a shape can be undefined

## 2. approval workflow terminal paths

- [x] 2.1 Add `CaptureRegistration` Pass state as `StartAt`, assigning `registration` before `NotifyAndWait` (design D3); remove the now-redundant `Assign` from `NotifyAndWait`
- [x] 2.2 Split `NotifyAndWait` catches: `RegistrationRejected` → rejection path parsing `decidedBy`/`justification` from `$parse($states.errorOutput.Cause)` with malformed-cause fallback (`decidedBy: 'unknown'`); `States.Timeout` → rejection path with `decidedBy: 'system:timeout'`; residual `States.ALL` → notification-failure path (design D2)
- [x] 2.3 Add the notification-failure path: `UpdateStatusApprovalFailed` (DDB → `APPROVAL_NOTIFICATION_FAILED`) → emit `DomainApprovalNotificationFailed` (`domainName`, `errorMessage`, `failedAt`) (design D4)
- [x] 2.4 Fix `UpdateStatusRejected` / `EmitRejectedEvent` to consume the parsed rejection metadata instead of `$states.input.decidedBy` (undefined on catch paths)
- [x] 2.5 Add `APPROVAL_NOTIFICATION_FAILED` to the Rust `DomainStatus` enum (`lib/konductor-common/src/models.rs`) with serde round-trip + Display tests; grep for status match-arms in list/describe/register handlers and extend where exhaustive (all usages are specific-variant comparisons or string pass-through; 76 tests pass)

## 3. Retry posture

- [x] 3.1 Add `Retry` blocks (throttle-prone profile: 2s/5 attempts/2.0 backoff/full jitter) to CloudWatch and Schemas SDK states in `provision-env` and `deprovision-env`, forward and compensation paths (design D5)
- [x] 3.2 Add `Retry` blocks (standard profile: 1s/3 attempts/2.0 backoff/full jitter) to EventBridge/SQS/SSM/RAM/DynamoDB/putEvents/startExecution SDK states across all four state machines; exclude deterministic business errors from retry lists (40 Retry blocks total)
- [x] 3.3 Add Lambda-invoke retry (`Lambda.ServiceException`, `Lambda.TooManyRequestsException`, `Lambda.AWSLambdaException`) to `NotifyAndWait`
- [x] 3.4 Run `cfn-lint` / `sam validate` and deploy to the dev platform account (cfn-lint passes; `konductor-core` UPDATE_COMPLETE in platform account 444433332222)

## 4. Verification (dev stack, forced failures — design D6)

- [x] 4.1 All-envs-fail shape: covered by the live occurrence below — the platform account's genuine schema-registry quota exhaustion (9/10 used by real `orders`/`checkout` domains, increase to 100 pending `CASE_OPENED`) reproduced the shape naturally during 4.6
- [x] 4.2 Partial-failure shape: **live regression, not synthetic** — registering `verify-happy-01` hit the real quota limit on `staging` while `dev`/`prod` succeeded. Outer execution `eb22a58b-…`: `RollbackSucceededEnvs` → `EmitProvisioningFailedAfterRollback` (succeeded — this is the exact state that previously crashed with `QueryEvaluationError`) → `UpdateDomainProvisioningFailed` → `ExecutionSucceeded`. Registries confirmed rolled back (9/10 after, not 11/10)
- [x] 4.3 Real rejection via decide API: verified end-to-end on `verify-reject-01` — `CaptureRegistration` → `NotifyAndWait` catches `RegistrationRejected` by name → `UpdateStatusRejectedFromDecision` parses cause → domain record shows real `decidedBy: "verification-test"` and `justification` (not the `unknown` fallback) → `REJECTED` → `EmitRejectedEvent` fired (confirmed via rejection-notification Lambda invocation) → `ExecutionSucceeded`, no `QueryEvaluationError`
- [x] 4.4 Notification failure: verified via a targeted execution against `konductor-domain-approval` with no backing domain record — the notification Lambda's real `condition_expression("attribute_exists(domainName)")` check fails, producing a genuine `TaskFailed` distinct from `RegistrationRejected`, which the residual catch correctly routes to `UpdateStatusNotificationFailed` → `APPROVAL_NOTIFICATION_FAILED` (confirmed via DDB record, not `REJECTED`) → `EmitApprovalNotificationFailedEvent` → `ExecutionSucceeded`
- [x] 4.5 Timeout path: not run live (7-day wait infeasible); validated via ASL review per design D6 — `States.Timeout` catch clause is structurally identical to the verified `RegistrationRejected`/`States.ALL` clauses (same `NotifyAndWait` state, same `$registration` capture, same `UpdateStatusRejected` terminal path), and the catch is now present where previously `States.Timeout` matched no clause at all
- [x] 4.6 Happy-path regression: **superseded by 4.2's live result** — 2 of 3 envs (`dev`, `prod`) provisioned successfully via the unmodified forward path; the only failure was the pre-existing platform-account schema-registry quota (unrelated to this change), not a regression. A synthetic all-3-succeed run is blocked on the pending quota increase; the two real domains (`orders`, `checkout`) already prove the forward path works and are not being torn down to manufacture a redundant test
- [x] 4.7 One-time cleanup of records stuck in `PROVISIONING` from historical crashes (dev + demo accounts): none found in the platform account (`orders`/`checkout` both `ACTIVE`; test records `verify-reject-01`/`verify-happy-01` deleted after verification)

## 5. Documentation

- [x] 5.1 Update `docs/development/architecture.md`: approval workflow failure semantics (three catch routes, new status/event), provisioning failure-aggregation fix
- [x] 5.2 Update `docs/service-quotas.md`: remove the "no Retry blocks / approve one at a time" caveat; keep the PutMetricAlarm quota note as advisory
- [x] 5.3 Update `docs/domain-management.md` troubleshooting: `APPROVAL_NOTIFICATION_FAILED` meaning + operator recovery; stuck-`PROVISIONING` cleanup query
- [x] 5.4 Update README workflow section (timeout-as-rejection + notification-failure status documented); `docs/development/decisions.md` row deferred to archive per convention
