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

The provisioning workflow SHALL create all domain resources in the correct order across both the platform account and the application (domain owner) account. The workflow SHALL set the domain status to `PROVISIONING` as its first step. The workflow SHALL detect whether the owner account is new. On success, the workflow SHALL create the namespace parameter, manage the RAM resource share, and set the domain status to `ACTIVE`. On failure, the workflow SHALL execute saga compensation to clean up created resources, then set the domain status to `PROVISIONING_FAILED`. The workflow SHALL be defined in YAML format and use JSONata as the query language.

#### Scenario: Provisioning workflow sets PROVISIONING status at start

- **WHEN** the provisioning workflow is triggered by a `DomainRegistrationApproved` event
- **THEN** the workflow SHALL update the domain record to `PROVISIONING` status as its first step before creating any resources

#### Scenario: Provisioning workflow detects new account

- **WHEN** the provisioning workflow sets `PROVISIONING` status
- **THEN** the workflow SHALL query the `by-owner` GSI for the `ownerAccountId` filtering for `ACTIVE` status
- **AND THEN** if no other `ACTIVE` domains exist for that account, the workflow SHALL flag the account as new in its execution context

#### Scenario: Provisioning workflow creates all resources

- **WHEN** the provisioning workflow proceeds after account detection
- **THEN** the workflow SHALL create the following resources in order:
  1. Domain event bus in the platform account with PutEvent policy restricting publishing to the domain owner account
  2. Schema registry in the platform account with access policies
  3. Subscription rule DLQ (SQS) in the platform account
  4. Domain event bus in the application (owner) account via assumed role
  5. Integration rule DLQ (SQS) in the application account via assumed role
  6. CloudWatch dashboard, alarms, and metrics configuration in the platform account
  7. SSM Advanced parameter at `/namespace/{domainName}` with value `{org}-{domainName}`
  8. RAM resource share association for the namespace parameter
- **AND THEN** upon successful completion, the workflow SHALL update the domain record to `ACTIVE` with all resource ARNs
- **AND THEN** the workflow SHALL emit a `DomainCreationSucceeded` event

#### Scenario: Provisioning workflow step failure triggers saga rollback

- **WHEN** any step in the provisioning workflow fails
- **THEN** the workflow SHALL execute the saga compensation chain to delete all successfully created resources in reverse order
- **AND THEN** the workflow SHALL update the domain record to `PROVISIONING_FAILED` with the error detail (including any compensation failures)
- **AND THEN** the workflow SHALL emit a `DomainCreationFailed` event with the error code, error message, and the name of the failed step

#### Scenario: Provisioning uses updated namespace convention for resource names

- **WHEN** the provisioning workflow creates domain resources
- **THEN** all resource names SHALL be derived from the `{org}-{env}-{domain}` namespace convention
- **AND THEN** the organisation, environment, and domain name values SHALL be sourced from the event detail and Nexus configuration

### Requirement: Provisioning workflow lifecycle events

The provisioning workflow SHALL emit lifecycle events to the Nexus event bus at key points: when provisioning starts, when it succeeds, and when it fails. These events SHALL conform to the event types and minimum payloads defined in the domain-management spec. The event source SHALL be `nexus.domain-management`.

#### Scenario: Provisioning started event

- **WHEN** the provisioning workflow begins execution
- **THEN** it SHALL emit a `DomainCreationStarted` event to the Nexus event bus with source `nexus.domain-management`, domainId, workflowExecutionId, startedAt, and isNewAccount flag

#### Scenario: Provisioning succeeded event

- **WHEN** the provisioning workflow completes all steps successfully
- **THEN** it SHALL emit a `DomainCreationSucceeded` event with source `nexus.domain-management`, domainId, workflowExecutionId, completedAt, resourceArns (map of all created resource ARNs), and namespaceParameter (SSM parameter path)

#### Scenario: Provisioning failed event

- **WHEN** the provisioning workflow fails at any step
- **THEN** it SHALL emit a `DomainCreationFailed` event with source `nexus.domain-management`, domainId, workflowExecutionId, failedAt, errorCode, errorMessage, and compensationResults (list of cleanup outcomes)

### Requirement: New account detection

The provisioning workflow SHALL detect whether the domain's `ownerAccountId` has been previously seen by querying the DynamoDB `by-owner` GSI. If no other domains with status `ACTIVE` exist for that account, the account SHALL be flagged as new. The new-account flag SHALL be included in the provisioning workflow's input context and surfaced in the approval notification email.

#### Scenario: First domain from a new account

- **WHEN** the provisioning workflow receives a `DomainRegistrationApproved` event for a domain whose `ownerAccountId` has no other `ACTIVE` domains in the domains table
- **THEN** the workflow SHALL flag the account as new
- **AND THEN** the approval notification email SHALL include a notice that trust-stack deployment is required for this account

#### Scenario: Subsequent domain from a known account

- **WHEN** the provisioning workflow receives a `DomainRegistrationApproved` event for a domain whose `ownerAccountId` already has at least one `ACTIVE` domain
- **THEN** the workflow SHALL NOT flag the account as new
- **AND THEN** provisioning SHALL proceed without any trust-stack advisory

#### Scenario: AssumeRole fails for new account without trust-stack

- **WHEN** the provisioning workflow attempts `AssumeAppAccountRole` for a new account and the trust role does not exist
- **THEN** the `AssumeRole` step SHALL fail
- **AND THEN** the saga compensation chain SHALL clean up all platform-side resources created before the failure
- **AND THEN** the domain SHALL be marked `PROVISIONING_FAILED` with an error detail indicating the trust role was not found and trust-stack deployment is required

### Requirement: Saga rollback on provisioning failure

The provisioning workflow SHALL implement compensating transactions using the saga pattern. When any resource creation step fails, the workflow SHALL delete all resources that were successfully created in reverse order before marking the domain as `PROVISIONING_FAILED`. Compensation SHALL be best-effort — individual compensation step failures SHALL be logged but SHALL NOT prevent subsequent compensation steps from executing.

#### Scenario: Failure after platform resources created but before app resources

- **WHEN** the provisioning workflow fails at `AssumeAppAccountRole` or `CreateAppEventBus`
- **THEN** the workflow SHALL delete the following platform resources in reverse order: subscription DLQ, schema registry, platform event bus (and any alarms/dashboards if created)
- **AND THEN** the domain SHALL be marked `PROVISIONING_FAILED` with the original error detail

#### Scenario: Failure after app resources created

- **WHEN** the provisioning workflow fails at `CreateDashboard` or any subsequent platform-side observability step
- **THEN** the workflow SHALL delete app-account resources (integration DLQ, app event bus) using the assumed role, then delete platform resources in reverse order
- **AND THEN** the domain SHALL be marked `PROVISIONING_FAILED` with the original error detail

#### Scenario: Compensation step fails

- **WHEN** a compensation step fails to delete a previously created resource
- **THEN** the workflow SHALL log the compensation failure with the resource identifier and error
- **AND THEN** the workflow SHALL continue to the next compensation step
- **AND THEN** the `PROVISIONING_FAILED` error detail SHALL include a list of resources that could not be cleaned up

#### Scenario: Full rollback on early failure

- **WHEN** the provisioning workflow fails at `CreatePlatformEventBus` (the first resource creation step)
- **THEN** no compensation SHALL be needed (no resources were created)
- **AND THEN** the domain SHALL be marked `PROVISIONING_FAILED` with the error detail

### Requirement: Namespace parameter creation

The provisioning workflow SHALL create an SSM Parameter Store Advanced parameter for the domain namespace during provisioning. The parameter SHALL be at path `/namespace/{domain}` with value `{org}-{domain}`. The parameter creation SHALL be idempotent — if the parameter already exists with the same value, no update SHALL occur.

#### Scenario: Namespace parameter created for new domain

- **WHEN** the provisioning workflow successfully creates all domain resources
- **THEN** the workflow SHALL create an SSM Advanced parameter at `/namespace/{domainName}` with the value `{org}-{domainName}`
- **AND THEN** the parameter SHALL be of type `String` and tier `Advanced`

#### Scenario: Namespace parameter already exists

- **WHEN** the provisioning workflow attempts to create a namespace parameter and a parameter at that path already exists with the same value
- **THEN** the workflow SHALL treat this as a successful no-op
- **AND THEN** provisioning SHALL continue to the next step

### Requirement: RAM resource share management

The provisioning workflow SHALL ensure that a RAM resource share exists for domain namespaces and that each new namespace parameter is associated with it. The RAM resource share SHALL be shared with the entire AWS Organization.

#### Scenario: RAM share created on first domain provisioning

- **WHEN** the provisioning workflow creates a namespace parameter and no RAM resource share named `{org}-domain-namespaces` exists
- **THEN** the workflow SHALL create a RAM resource share named `{org}-domain-namespaces`
- **AND THEN** the share SHALL have the AWS Organization as its principal
- **AND THEN** the new namespace parameter SHALL be associated with the share

#### Scenario: Namespace parameter added to existing RAM share

- **WHEN** the provisioning workflow creates a namespace parameter and a RAM resource share named `{org}-domain-namespaces` already exists
- **THEN** the workflow SHALL associate the new namespace parameter with the existing share

#### Scenario: RAM share association is idempotent

- **WHEN** the provisioning workflow attempts to associate a namespace parameter that is already in the RAM share
- **THEN** the association SHALL be treated as a successful no-op

