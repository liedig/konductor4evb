# Tasks: harden-deprovision-idempotency

## 1. Workflow Error Taxonomy

- [x] 1.1 Add service-specific not-found catches (silent no-op, ordered before the `States.ALL` catch) to each step in `deprovision-env.asl.yaml`: schema registry (`Schemas.NotFoundException`), event buses (`EventBridge.ResourceNotFoundException` — the aws-sdk integration uses the `EventBridge.` service prefix, matching the existing retry config), dashboard (CloudWatch `ResourceNotFound`/`ResourceNotFoundException`), SQS queues (`Sqs.QueueDoesNotExistException`/`Sqs.QueueDoesNotExist`). CloudWatch `deleteAlarms` is already a silent no-op on missing alarms, so the two alarm steps need no not-found catch.
- [x] 1.2 Add input-presence guards so `DeleteIntegrationDlq` and `DeleteSubscriptionDlq` skip as no-ops when their queue URL is absent from `resourceArns` — implemented as `CheckIntegrationDlqPresent` and `CheckSubscriptionDlqPresent` Choice states using `$exists($states.input.resourceArns.<arn>)`.
- [x] 1.3 Update the workflow's header comment: `compensationErrors` non-empty means resources were left behind

## 2. Static Validation

- [x] 2.1 cfn-lint / `sam validate --lint` pass

## 3. Verify in Dev

- [ ] 3.1 Not-found path: run deprovision twice against the same env (second run must SUCCEED with empty `compensationErrors`)
- [ ] 3.2 Partial-input path: invoke with a `resourceArns` set missing the queue URLs and confirm empty `compensationErrors`
- [ ] 3.3 Genuine-failure path: invoke against a resource the role cannot delete (or a simulated denial) and confirm the error IS recorded in `compensationErrors`
- [ ] 3.4 Confirm the exact not-found exception names surfaced by the aws-sdk integrations match the catch lists (adjust if live behavior differs); resolve the design's open question
- [ ] 3.5 End-to-end: `make teardown-domain` on a fully provisioned domain reports success and deletes the record; on a never-provisioned (rejected) domain it skips envs and deletes the record
