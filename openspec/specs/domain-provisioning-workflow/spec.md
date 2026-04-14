# domain-provisioning-workflow Specification

## Purpose
TBD - created by archiving change architecture-decisions. Update Purpose after archive.
## Requirements
### Requirement: Two-phase workflow separation

Domain registration SHALL be handled by two separate AWS Step Functions workflows: an approval workflow and a provisioning workflow. The approval workflow SHALL manage the human-in-the-loop approval process including notification, decision handling, and status updates. The provisioning workflow SHALL create all domain resources. The two workflows SHALL communicate via lifecycle events on the control plane event bus. Both workflows SHALL be defined in YAML format and use JSONata as the query language.

#### Scenario: Approval workflow completes and triggers provisioning

- **WHEN** the approval workflow determines that a domain registration request is approved
- **THEN** the approval workflow SHALL update the domain record to `APPROVED` status in DynamoDB
- **AND THEN** emit a `DomainRegistrationApproved` event to the control plane event bus
- **AND THEN** an EventBridge rule on the control plane bus SHALL trigger the provisioning workflow in response to that event
- **AND THEN** the approval workflow and provisioning workflow SHALL be independent Step Functions state machines

#### Scenario: Approval workflow rejects request

- **WHEN** the approval workflow determines that a domain registration request is rejected
- **THEN** the approval workflow SHALL update the domain record to `REJECTED` status in DynamoDB with `decidedBy`, `justification`, and `rejectedAt`
- **AND THEN** emit a `DomainRegistrationRejected` event to the control plane event bus
- **AND THEN** no provisioning workflow SHALL be triggered

### Requirement: Human-in-the-loop approval

The approval workflow SHALL include a human approval step using the Step Functions `.waitForTaskToken` pattern. When a registration request is submitted, the workflow SHALL invoke a notification Lambda that persists the task token and sends an email, then block until a callback is received. The control plane SHALL provide a synchronous API endpoint for approvers to submit their decision along with their identity and justification.

#### Scenario: Approval workflow notifies and waits for human decision

- **WHEN** a new domain registration request triggers the approval workflow
- **THEN** the workflow SHALL enter the `NotifyAndWait` state using the `.waitForTaskToken` resource
- **AND THEN** the state SHALL invoke the `SendApprovalNotification` Lambda with the task token (`$states.context.Task.Token`), domain ID, domain name, owner account ID, and owner email
- **AND THEN** the Lambda SHALL persist the task token to the domain record in DynamoDB and send an approval notification email via SES
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
- **THEN** the decide Lambda SHALL call `sfn:SendTaskFailure` with error `RegistrationRejected` and cause containing `decidedBy` and `justification`
- **AND THEN** the approval workflow SHALL catch the `States.TaskFailed` error
- **AND THEN** the workflow SHALL update the domain record to `REJECTED` status with `decidedBy`, `justification`, and `rejectedAt`
- **AND THEN** the workflow SHALL emit the `DomainRegistrationRejected` event

### Requirement: Provisioning workflow resource creation

The provisioning workflow SHALL create all domain resources in the correct order across both the platform account and the application (domain owner) account. The workflow SHALL set the domain status to `PROVISIONING` as its first step. On success, the workflow SHALL set the domain status to `ACTIVE`. On failure, the workflow SHALL set the domain status to `PROVISIONING_FAILED`. The workflow SHALL be defined in YAML format and use JSONata as the query language.

#### Scenario: Provisioning workflow sets PROVISIONING status at start

- **WHEN** the provisioning workflow is triggered by a `DomainRegistrationApproved` event
- **THEN** the workflow SHALL update the domain record to `PROVISIONING` status as its first step before creating any resources

#### Scenario: Provisioning workflow creates all resources

- **WHEN** the provisioning workflow proceeds after setting `PROVISIONING` status
- **THEN** the workflow SHALL create the following resources in order:
  1. Domain event bus in the platform account with PutEvent policy restricting publishing to the domain owner account
  2. Schema registry in the platform account with access policies
  3. Subscription rule DLQ (SQS) in the platform account
  4. Domain event bus in the application (owner) account via assumed role
  5. Integration rule DLQ (SQS) in the application account via assumed role
  6. CloudWatch dashboard, alarms, and metrics configuration in the platform account
- **AND THEN** upon successful completion, the workflow SHALL update the domain record to `ACTIVE` with all resource ARNs
- **AND THEN** the workflow SHALL emit a `DomainCreationSucceeded` event

#### Scenario: Provisioning workflow step failure

- **WHEN** any step in the provisioning workflow fails
- **THEN** the workflow SHALL update the domain record to `PROVISIONING_FAILED` with the error detail
- **AND THEN** the workflow SHALL emit a `DomainCreationFailed` event with the error code, error message, and the name of the failed step
- **AND THEN** resources created by earlier steps SHALL be retained for diagnosis (not automatically rolled back)

#### Scenario: Provisioning uses namespace convention for resource names

- **WHEN** the provisioning workflow creates domain resources
- **THEN** all resource names SHALL be derived from the configured namespace convention (default: `{env}-{org}-{domain}`)
- **AND THEN** the environment, organization, and domain name values SHALL be sourced from the event detail and control plane configuration

### Requirement: Provisioning workflow lifecycle events

The provisioning workflow SHALL emit lifecycle events to the control plane event bus at key points: when provisioning starts, when it succeeds, and when it fails. These events SHALL conform to the event types and minimum payloads defined in the domain-management spec.

#### Scenario: Provisioning started event

- **WHEN** the provisioning workflow begins execution
- **THEN** it SHALL emit a `DomainCreationStarted` event to the control plane event bus with: domainId, workflowExecutionId, and startedAt

#### Scenario: Provisioning succeeded event

- **WHEN** the provisioning workflow completes all steps successfully
- **THEN** it SHALL emit a `DomainCreationSucceeded` event with: domainId, workflowExecutionId, completedAt, and resourceArns (map of all created resource ARNs)

#### Scenario: Provisioning failed event

- **WHEN** the provisioning workflow fails at any step
- **THEN** it SHALL emit a `DomainCreationFailed` event with: domainId, workflowExecutionId, failedAt, errorCode, and errorMessage

