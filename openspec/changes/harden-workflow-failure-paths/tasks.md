# Tasks: harden-workflow-failure-paths

## 1. provision-domain failure aggregation

- [ ] 1.1 Replace the `compensationErrors` expression in `EmitProvisioningFailedAfterRollback` with the array-constructor form (`[$rollbackResults.compensationErrors]`); initialize `rollbackResults` to `[]` where `failedResults`/`succeededResults` are assigned (defensive per design D1)
- [ ] 1.2 Audit the remaining JSONata in `EmitProvisioningFailedAfterRollback` and `EmitProvisioningFailedDomainLevel` for the same undefined-propagation class (`$failedResults[0].…` when failure detail is absent) and guard where a shape can be undefined

## 2. approval workflow terminal paths

- [ ] 2.1 Add `CaptureRegistration` Pass state as `StartAt`, assigning `registration` before `NotifyAndWait` (design D3); remove the now-redundant `Assign` from `NotifyAndWait`
- [ ] 2.2 Split `NotifyAndWait` catches: `RegistrationRejected` → rejection path parsing `decidedBy`/`justification` from `$parse($states.errorOutput.Cause)` with malformed-cause fallback (`decidedBy: 'unknown'`); `States.Timeout` → rejection path with `decidedBy: 'system:timeout'`; residual `States.ALL` → notification-failure path (design D2)
- [ ] 2.3 Add the notification-failure path: `UpdateStatusApprovalFailed` (DDB → `APPROVAL_NOTIFICATION_FAILED`) → emit `DomainApprovalNotificationFailed` (`domainName`, `errorMessage`, `failedAt`) (design D4)
- [ ] 2.4 Fix `UpdateStatusRejected` / `EmitRejectedEvent` to consume the parsed rejection metadata instead of `$states.input.decidedBy` (undefined on catch paths)
- [ ] 2.5 Add `APPROVAL_NOTIFICATION_FAILED` to the Rust `DomainStatus` enum (`lib/nexus-common/src/models.rs`) with serde round-trip + Display tests; grep for status match-arms in list/describe/register handlers and extend where exhaustive

## 3. Retry posture

- [ ] 3.1 Add `Retry` blocks (throttle-prone profile: 2s/5 attempts/2.0 backoff/full jitter) to CloudWatch and Schemas SDK states in `provision-env` and `deprovision-env`, forward and compensation paths (design D5)
- [ ] 3.2 Add `Retry` blocks (standard profile: 1s/3 attempts/2.0 backoff/full jitter) to EventBridge/SQS/SSM/RAM/DynamoDB/putEvents/startExecution SDK states across all four state machines; exclude deterministic business errors from retry lists
- [ ] 3.3 Add Lambda-invoke retry (`Lambda.ServiceException`, `Lambda.TooManyRequestsException`, `Lambda.AWSLambdaException`) to `NotifyAndWait`
- [ ] 3.4 Run `cfn-lint` / `sam validate` and deploy to the dev platform account

## 4. Verification (dev stack, forced failures — design D6)

- [ ] 4.1 All-envs-fail shape: force schema-registry quota exhaustion, approve a domain, assert domain reaches `PROVISIONING_FAILED`, `DomainProvisioningFailed` emitted with `compensationErrors: []`, no `QueryEvaluationError` in execution history
- [ ] 4.2 Partial-failure shape: poison one env, assert clean sibling rollback, `PROVISIONING_FAILED`, event emitted with empty compensation errors
- [ ] 4.3 Real rejection via decide API: assert `REJECTED` with parsed `decidedBy`/`justification`, `DomainRegistrationRejected` emitted
- [ ] 4.4 Notification failure: deploy with unverified sender, register a domain, assert `APPROVAL_NOTIFICATION_FAILED` (not `REJECTED`) and `DomainApprovalNotificationFailed` emitted
- [ ] 4.5 Timeout path: dev-only deploy with short `TimeoutSeconds`, assert `REJECTED` with `decidedBy: system:timeout`
- [ ] 4.6 Happy-path regression: register → approve → `ACTIVE` end-to-end still works
- [ ] 4.7 One-time cleanup of records stuck in `PROVISIONING` from historical crashes (dev + demo accounts)

## 5. Documentation

- [ ] 5.1 Update `docs/development/architecture.md`: approval workflow failure semantics (three catch routes, new status/event), provisioning failure-aggregation fix
- [ ] 5.2 Update `docs/service-quotas.md`: remove the "no Retry blocks / approve one at a time" caveat; keep the PutMetricAlarm quota note as advisory
- [ ] 5.3 Update `docs/domain-management.md` troubleshooting: `APPROVAL_NOTIFICATION_FAILED` meaning + operator recovery; stuck-`PROVISIONING` cleanup query
- [ ] 5.4 Update README workflow section (timeout-as-rejection now actually implemented) and `docs/development/decisions.md` row on archive
