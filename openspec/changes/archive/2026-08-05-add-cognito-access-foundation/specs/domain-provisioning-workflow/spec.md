## MODIFIED Requirements

### Requirement: Human-in-the-loop approval

The approval workflow SHALL include a human approval step using the Step Functions `.waitForTaskToken` pattern. When a registration request is submitted, the workflow SHALL capture the registration context in a workflow variable before entering the task-token wait, so the context is available on every downstream path including error catches. The workflow SHALL invoke a notification Lambda that persists the task token and sends an approval notification, then block until a callback is received.

The control plane SHALL provide a synchronous OAuth-only API endpoint for an authenticated `platform-admin` to submit a decision and justification. The task token SHALL remain workflow callback state and possession of it SHALL NOT establish caller authorization. The endpoint SHALL derive the decider identity from trusted OAuth principal context. The wait state SHALL distinguish three failure outcomes with separate catch routes: human rejection (custom error `RegistrationRejected`), timeout (`States.Timeout`), and notification-infrastructure failure (residual errors). Transient Lambda invocation errors SHALL be retried before any catch route is taken.

#### Scenario: Approval workflow notifies and waits for human decision

- **WHEN** a new domain registration request triggers the approval workflow
- **THEN** the workflow SHALL first capture the registration input in a workflow variable via a Pass state
- **AND THEN** the workflow SHALL enter the `NotifyAndWait` state using the `.waitForTaskToken` resource
- **AND THEN** the state SHALL invoke the `SendApprovalNotification` Lambda with the task token (`$states.context.Task.Token`), domain name, owner account ID, and owner email
- **AND THEN** the Lambda SHALL persist the task token to the domain record in DynamoDB and send an approval notification that directs the approver to an authenticated experience
- **AND THEN** the workflow SHALL block until a `SendTaskSuccess` or `SendTaskFailure` callback is received
- **AND THEN** the workflow SHALL timeout after 7 days if no callback is received

#### Scenario: Authenticated approver approves the request

- **WHEN** an OAuth-authenticated `platform-admin` calls `POST /domains/registrations/decide` with the task token and decision `ACCEPT`
- **THEN** the decide Lambda SHALL call `sfn:SendTaskSuccess` with callback output containing `approved: true`, trusted principal-derived `decidedBy`, and `justification`
- **AND THEN** the approval workflow SHALL resume with the callback output as state input
- **AND THEN** the workflow SHALL update the domain record to `APPROVED` status with `decidedBy`, `justification`, and `approvedAt`
- **AND THEN** the workflow SHALL emit the `DomainRegistrationApproved` event

#### Scenario: Authenticated approver rejects the request

- **WHEN** an OAuth-authenticated `platform-admin` calls `POST /domains/registrations/decide` with the task token and decision `REJECT`
- **THEN** the decide Lambda SHALL call `sfn:SendTaskFailure` with error `RegistrationRejected` and cause containing trusted principal-derived `decidedBy` and `justification`
- **AND THEN** the approval workflow SHALL catch the `RegistrationRejected` error by name rather than generic `States.TaskFailed`
- **AND THEN** the workflow SHALL parse `decidedBy` and `justification` from the error cause JSON
- **AND THEN** the workflow SHALL update the domain record to `REJECTED` status with the parsed `decidedBy`, `justification`, and `rejectedAt`
- **AND THEN** the workflow SHALL emit the `DomainRegistrationRejected` event with the parsed decision metadata

#### Scenario: Task token is presented without OAuth authorization

- **WHEN** a caller presents a valid task token without a trusted OAuth `platform-admin` principal
- **THEN** the callback API SHALL reject the request before invoking the decide Lambda
- **AND THEN** the approval workflow SHALL remain waiting

#### Scenario: Rejection cause is malformed

- **WHEN** the workflow catches `RegistrationRejected` but the error cause is not parseable as JSON
- **THEN** the workflow SHALL still complete the rejection path with `decidedBy` falling back to `unknown` and an empty justification
- **AND THEN** the domain record SHALL reach `REJECTED` and the `DomainRegistrationRejected` event SHALL be emitted

#### Scenario: Approval request times out

- **WHEN** no decision callback is received within 7 days and the wait state raises `States.Timeout`
- **THEN** the workflow SHALL catch `States.Timeout` with a dedicated catch clause
- **AND THEN** the workflow SHALL update the domain record to `REJECTED` with `decidedBy` set to `system:timeout` and a fixed justification indicating expiry
- **AND THEN** the workflow SHALL emit the `DomainRegistrationRejected` event

#### Scenario: Notification delivery fails

- **WHEN** the notification Lambda fails after invocation retries are exhausted
- **THEN** the workflow SHALL route to a notification-failure path distinct from rejection
- **AND THEN** the domain record SHALL be updated to `APPROVAL_NOTIFICATION_FAILED`, not `REJECTED`
- **AND THEN** the workflow SHALL emit a `DomainApprovalNotificationFailed` event with `domainName` and the error message
- **AND THEN** no `DomainRegistrationRejected` event SHALL be emitted

#### Scenario: Every approval execution path terminates deterministically

- **WHEN** the approval workflow completes by any path—approval, rejection, timeout, or notification failure
- **THEN** the domain record SHALL be left in a defined terminal status for that path (`APPROVED`, `REJECTED`, or `APPROVAL_NOTIFICATION_FAILED`)
- **AND THEN** exactly one lifecycle event SHALL have been emitted
- **AND THEN** the execution SHALL NOT fail with `States.QueryEvaluationError`
