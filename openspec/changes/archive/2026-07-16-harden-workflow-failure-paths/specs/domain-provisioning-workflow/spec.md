# Capability: Domain Provisioning Workflow (delta)

## MODIFIED Requirements

### Requirement: Human-in-the-loop approval

The approval workflow SHALL include a human approval step using the Step Functions `.waitForTaskToken` pattern. When a registration request is submitted, the workflow SHALL capture the registration context in a workflow variable **before** entering the task-token wait (in a state that cannot fail), so the context is available on every downstream path including error catches. The workflow SHALL invoke a notification Lambda that persists the task token and sends an email, then block until a callback is received. The control plane SHALL provide a synchronous API endpoint for approvers to submit their decision along with their identity and justification. The wait state SHALL distinguish three failure outcomes with separate catch routes: human rejection (custom error `RegistrationRejected`), timeout (`States.Timeout`), and notification-infrastructure failure (residual errors). Transient Lambda invocation errors SHALL be retried before any catch route is taken.

#### Scenario: Approval workflow notifies and waits for human decision

- **WHEN** a new domain registration request triggers the approval workflow
- **THEN** the workflow SHALL first capture the registration input in a workflow variable via a Pass state
- **AND THEN** the workflow SHALL enter the `NotifyAndWait` state using the `.waitForTaskToken` resource
- **AND THEN** the state SHALL invoke the `SendApprovalNotification` Lambda with the task token (`$states.context.Task.Token`), domain name, owner account ID, and owner email
- **AND THEN** the Lambda SHALL persist the task token to the domain record in DynamoDB (keyed by `domainName`) and send an approval notification email via SES
- **AND THEN** the workflow SHALL block until a `SendTaskSuccess` or `SendTaskFailure` callback is received
- **AND THEN** the workflow SHALL timeout after 7 days if no callback is received

#### Scenario: Approver approves the request

- **WHEN** an authorized approver calls `POST /domains/registrations/decide` with the task token and decision `ACCEPT`
- **THEN** the decide Lambda SHALL call `sfn:SendTaskSuccess` with the callback output containing `approved: true`, `decidedBy`, and `justification`
- **AND THEN** the approval workflow SHALL resume with the callback output as state input
- **AND THEN** the workflow SHALL update the domain record to `APPROVED` status with `decidedBy`, `justification`, and `approvedAt`
- **AND THEN** the workflow SHALL emit the `DomainRegistrationApproved` event

#### Scenario: Approver rejects the request

- **WHEN** an authorized approver calls `POST /domains/registrations/decide` with the task token and decision `REJECT`
- **THEN** the decide Lambda SHALL call `sfn:SendTaskFailure` with error `RegistrationRejected` and cause containing a JSON object with `decidedBy` and `justification`
- **AND THEN** the approval workflow SHALL catch the `RegistrationRejected` error by name (not the generic `States.TaskFailed`)
- **AND THEN** the workflow SHALL parse `decidedBy` and `justification` from the error cause JSON
- **AND THEN** the workflow SHALL update the domain record to `REJECTED` status with the parsed `decidedBy`, `justification`, and `rejectedAt`
- **AND THEN** the workflow SHALL emit the `DomainRegistrationRejected` event with the parsed decision metadata

#### Scenario: Rejection cause is malformed

- **WHEN** the workflow catches `RegistrationRejected` but the error cause is not parseable as JSON (e.g., a manual `SendTaskFailure` issued via CLI)
- **THEN** the workflow SHALL still complete the rejection path with `decidedBy` falling back to `unknown` and an empty justification
- **AND THEN** the domain record SHALL reach `REJECTED` and the `DomainRegistrationRejected` event SHALL be emitted

#### Scenario: Approval request times out

- **WHEN** no decision callback is received within 7 days and the wait state raises `States.Timeout`
- **THEN** the workflow SHALL catch `States.Timeout` with a dedicated catch clause
- **AND THEN** the workflow SHALL update the domain record to `REJECTED` with `decidedBy` set to `system:timeout` and a fixed justification indicating expiry
- **AND THEN** the workflow SHALL emit the `DomainRegistrationRejected` event

#### Scenario: Notification delivery fails

- **WHEN** the notification Lambda fails after invocation retries are exhausted (e.g., SES identity unverified)
- **THEN** the workflow SHALL route to a notification-failure path distinct from rejection
- **AND THEN** the domain record SHALL be updated to `APPROVAL_NOTIFICATION_FAILED` — NOT `REJECTED`
- **AND THEN** the workflow SHALL emit a `DomainApprovalNotificationFailed` event with `domainName` and the error message
- **AND THEN** no `DomainRegistrationRejected` event SHALL be emitted

#### Scenario: Every approval execution path terminates deterministically

- **WHEN** the approval workflow completes by any path — approval, rejection, timeout, or notification failure
- **THEN** the domain record SHALL be left in a defined terminal status for that path (`APPROVED`, `REJECTED`, or `APPROVAL_NOTIFICATION_FAILED`)
- **AND THEN** exactly one lifecycle event SHALL have been emitted
- **AND THEN** the execution SHALL NOT fail with `States.QueryEvaluationError`

### Requirement: Multi-environment provisioning coordinator

Domain provisioning SHALL be coordinated by an outer Step Functions workflow (`provision-domain`) that orchestrates per-environment provisioning via an embedded inner workflow (`provision-env`). The outer workflow SHALL run per-environment provisioning in parallel using a Map state. The outer workflow SHALL enforce an all-or-nothing contract across environments: if any environment fails to provision, the domain SHALL be marked `PROVISIONING_FAILED` and all environments that provisioned successfully SHALL be torn down via the `deprovision-env` workflow. The domain namespace SSM parameter and RAM resource share SHALL be managed by the outer workflow at the domain level, above the Map state, and SHALL NOT be rolled back on provisioning failure. The failure-handling chain SHALL be total: the terminal status update and failure event SHALL be reached for every failure shape, including when zero environments succeeded (empty rollback set) and when rollback completes with no compensation errors.

#### Scenario: Outer workflow orchestrates per-env provisioning

- **WHEN** the outer provisioning workflow is triggered by a `DomainRegistrationApproved` event
- **THEN** it SHALL set the domain status to `PROVISIONING` as its first step
- **AND THEN** it SHALL detect whether the `ownerAccountId` is a new account (as defined in the existing new-account-detection scenarios)
- **AND THEN** it SHALL create the SSM namespace parameter and manage the RAM resource share at the domain level
- **AND THEN** it SHALL start a Map state that, for each environment in the domain's `environments` list, starts a `provision-env` execution via `StartExecution.sync:2`
- **AND THEN** the Map state SHALL run with unbounded concurrency (all environments in parallel)

#### Scenario: All environments succeed

- **WHEN** every `provision-env` execution in the Map state completes successfully
- **THEN** the outer workflow SHALL update the domain record to `ACTIVE` with a `resourceArns` map keyed by environment, each value containing the per-environment resource ARNs returned by the inner workflow
- **AND THEN** the outer workflow SHALL emit a `DomainProvisioningSucceeded` event with the domain name, workflow execution ID, completion timestamp, environments list, per-environment resource ARNs map, namespace parameter path, and `isNewAccount` flag

#### Scenario: Any environment fails — succeeded siblings are torn down

- **WHEN** one or more `provision-env` executions fail
- **THEN** the outer workflow SHALL run a second Map state in parallel over the environments that succeeded
- **AND THEN** each iteration SHALL start a `deprovision-env` execution via `StartExecution.sync:2` to delete that environment's resources
- **AND THEN** the outer workflow SHALL update the domain record to `PROVISIONING_FAILED`
- **AND THEN** the outer workflow SHALL emit a `DomainProvisioningFailed` event populated with the first failed environment, the step within that environment that failed, the error code and message, and any compensation errors collected from both the inner saga and the outer sibling teardown

#### Scenario: All environments fail — empty rollback set handled

- **WHEN** every `provision-env` execution fails (e.g., a shared quota such as the schema-registry limit affects all environments)
- **THEN** the sibling-teardown Map state SHALL run over an empty set and complete
- **AND THEN** the outer workflow SHALL update the domain record to `PROVISIONING_FAILED`
- **AND THEN** the outer workflow SHALL emit `DomainProvisioningFailed` with `compensationErrors` as an empty array
- **AND THEN** the execution SHALL NOT fail with `States.QueryEvaluationError`

#### Scenario: Rollback completes cleanly — empty compensation errors handled

- **WHEN** sibling teardown runs and every `deprovision-env` execution completes without recording compensation errors
- **THEN** the outer workflow SHALL emit `DomainProvisioningFailed` with `compensationErrors` as an empty array
- **AND THEN** the domain record SHALL reach `PROVISIONING_FAILED`

#### Scenario: Namespace and RAM share persist across retries

- **WHEN** the outer provisioning workflow fails after the SSM namespace parameter and RAM share have already been created
- **THEN** the namespace parameter SHALL remain in place
- **AND THEN** the RAM share association SHALL remain in place
- **AND THEN** a subsequent retry of provisioning for the same domain SHALL reuse the existing namespace and RAM share as idempotent no-ops

## ADDED Requirements

### Requirement: Workflow retry posture for transient service errors

Every AWS SDK task state in the domain-management state machines (`approval`, `provision-domain`, `provision-env`, `deprovision-env`) SHALL declare a `Retry` policy, evaluated before any `Catch`, covering throttling and transient service errors (throttle exceptions, `ServiceUnavailable`, internal service errors) with exponential backoff and jitter. Throttle-prone APIs (CloudWatch alarm/dashboard operations, Schemas registry operations) SHALL use a longer retry budget than standard control-plane APIs. Business errors that indicate a deterministic outcome (e.g., resource-already-exists, conditional-check failures) SHALL NOT be retried and SHALL fail fast into the existing catch/compensation logic. Compensation and teardown task states SHALL carry the same retry policies as forward-path states.

#### Scenario: Throttled provisioning step retries instead of failing the environment

- **WHEN** a provisioning SDK call (e.g., `cloudwatch:putMetricAlarm` during concurrent multi-environment provisioning) receives a throttling error
- **THEN** the task SHALL retry with exponential backoff and jitter up to its retry budget
- **AND THEN** if a retry succeeds, provisioning SHALL continue with no compensation triggered
- **AND THEN** only after the retry budget is exhausted SHALL the error propagate to the catch and trigger the environment's saga rollback

#### Scenario: Deterministic business error fails fast

- **WHEN** a provisioning SDK call fails with a non-transient error (e.g., `ResourceAlreadyExistsException`)
- **THEN** the task SHALL NOT retry
- **AND THEN** the error SHALL propagate immediately to the existing catch/compensation path

#### Scenario: Throttled compensation step retries before recording a compensation error

- **WHEN** a teardown SDK call during rollback receives a throttling error
- **THEN** the task SHALL retry with the same policy as forward-path states
- **AND THEN** a compensation error SHALL be recorded only if the retry budget is exhausted
