# Tasks: harden-deprovision-idempotency

## 1. Workflow Error Taxonomy

- [x] 1.1 Add service-specific not-found catches (silent no-op, ordered before the `States.ALL` catch) to each step in `deprovision-env.asl.yaml`: schema registry (`Schemas.NotFoundException`), event buses (`EventBridge.ResourceNotFoundException` — the aws-sdk integration uses the `EventBridge.` service prefix, matching the existing retry config), dashboard (CloudWatch `ResourceNotFound`/`ResourceNotFoundException`), SQS queues (`Sqs.QueueDoesNotExistException`/`Sqs.QueueDoesNotExist`). CloudWatch `deleteAlarms` is already a silent no-op on missing alarms, so the two alarm steps need no not-found catch.
- [x] 1.2 Add input-presence guards so `DeleteIntegrationDlq` and `DeleteSubscriptionDlq` skip as no-ops when their queue URL is absent from `resourceArns` — implemented as `CheckIntegrationDlqPresent` and `CheckSubscriptionDlqPresent` Choice states using `$exists($states.input.resourceArns.<arn>)`.
- [x] 1.3 Update the workflow's header comment: `compensationErrors` non-empty means resources were left behind

## 2. Static Validation

- [x] 2.1 cfn-lint / `sam validate --lint` pass

## 3. Verify in Dev

- [x] 3.1 Not-found path: run deprovision twice against the same env (second run must SUCCEED with empty `compensationErrors`) — verified on dev: first deprovision via `make teardown-domain` (all resources existed), second direct invocation with the same input (all resources gone) returned `SUCCEEDED` with `compensationErrors: []`
- [x] 3.2 Partial-input path: invoke with a `resourceArns` set missing the queue URLs and confirm empty `compensationErrors` — verified on dev: `CheckIntegrationDlqPresent` and `CheckSubscriptionDlqPresent` Choice states both routed to their Default branches, skipping the two queue deletes; `compensationErrors: []`
- [x] 3.3 Genuine-failure path: invoke against a resource the role cannot delete (or a simulated denial) and confirm the error IS recorded in `compensationErrors` — verified on dev with `ownerAccountId: 111122223333`: `DeleteIntegrationDlq` and `DeleteAppEventBus` (the only cross-account steps using `Credentials.RoleArn`) failed with `AccessDenied` on assume-role and both errors were recorded; `compensationErrors` had 2 entries
- [x] 3.4 Confirm the exact not-found exception names surfaced by the aws-sdk integrations match the catch lists (adjust if live behavior differs); resolve the design's open question — confirmed via execution history: SQS throws `Sqs.QueueDoesNotExistException` (matches catch), Schemas throws `Schemas.NotFoundException` (matches catch). EventBridge `deleteEventBus` and CloudWatch `deleteDashboards` on missing resources return **silent success** (no exception thrown) — the defensive `EventBridge.ResourceNotFoundException` and `CloudWatch.ResourceNotFound` catches are unused but harmless. Design's open question resolved: the only not-found exceptions that actually fire are SQS and Schemas.
- [x] 3.5 End-to-end: `make teardown-domain` on a fully provisioned domain reports success and deletes the record; on a never-provisioned (rejected) domain it skips envs and deletes the record — first half verified on `smoke-test` (fully provisioned, ACTIVE across dev/staging/prod): all 3 envs deprovisioned cleanly, record deleted, no orphaned resources. Second half verified with a synthetic `REJECTED` record (`verify-skip-test`) with empty `resourceArns.M.<env>.M` for all 3 envs: each env printed "no provisioned resources on record (never provisioned) — skipping" and the record was deleted.
