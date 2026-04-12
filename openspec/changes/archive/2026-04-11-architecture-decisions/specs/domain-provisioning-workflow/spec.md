## ADDED Requirements

### Requirement: Two-phase workflow separation

Domain registration SHALL be handled by two separate AWS Step Functions workflows: an approval workflow and a provisioning workflow. The approval workflow SHALL manage the human-in-the-loop approval process. The provisioning workflow SHALL create all domain resources. The two workflows SHALL communicate via lifecycle events on the control plane event bus.

#### Scenario: Approval workflow completes and triggers provisioning

- **WHEN** the approval workflow determines that a domain registration request is approved
- **THEN** the approval workflow SHALL emit a `DomainRegistrationApproved` event to the control plane event bus
- **AND THEN** an EventBridge rule on the control plane bus SHALL trigger the provisioning workflow in response to that event
- **AND THEN** the approval workflow and provisioning workflow SHALL be independent Step Functions state machines

#### Scenario: Approval workflow rejects request

- **WHEN** the approval workflow determines that a domain registration request is rejected
- **THEN** the approval workflow SHALL emit a `DomainRegistrationRejected` event to the control plane event bus
- **AND THEN** no provisioning workflow SHALL be triggered
- **AND THEN** the control plane SHALL update the registration request status in DynamoDB to rejected

### Requirement: Human-in-the-loop approval

The approval workflow SHALL include a human approval step using Step Functions Task Token pattern. When a registration request is submitted, the workflow SHALL pause and wait for a human approver (platform team member) to approve or reject the request. The control plane SHALL provide an API endpoint for approvers to submit their decision along with the task token.

#### Scenario: Approval workflow pauses for human decision

- **WHEN** a new domain registration request is submitted via the RegisterDomain API
- **THEN** the control plane SHALL start the approval workflow
- **AND THEN** the workflow SHALL enter a wait state using a Task Token
- **AND THEN** the task token and request details SHALL be persisted so the approver can act on them later

#### Scenario: Approver approves the request

- **WHEN** an authorized approver calls the approval API endpoint with the task token and an approval decision
- **THEN** the approval workflow SHALL resume with the approval signal
- **AND THEN** the workflow SHALL update the request status in DynamoDB to Approved
- **AND THEN** the workflow SHALL emit the `DomainRegistrationApproved` event

#### Scenario: Approver rejects the request

- **WHEN** an authorized approver calls the approval API endpoint with the task token and a rejection decision
- **THEN** the approval workflow SHALL resume with the rejection signal
- **AND THEN** the workflow SHALL update the request status in DynamoDB to Rejected
- **AND THEN** the workflow SHALL emit the `DomainRegistrationRejected` event with an optional reason

### Requirement: Provisioning workflow resource creation

The provisioning workflow SHALL create all domain resources in the correct order across both the platform account and the application (domain owner) account. The workflow SHALL create resources in the platform account directly and in the application account via assumed roles.

#### Scenario: Provisioning workflow creates all resources

- **WHEN** the provisioning workflow is triggered by a `DomainRegistrationApproved` event
- **THEN** the workflow SHALL create the following resources in order:
  1. Domain event bus in the platform account with PutEvent policy restricting publishing to the domain owner account
  2. Schema registry in the platform account with access policies
  3. Subscription rule DLQ (SQS) in the platform account
  4. Domain event bus in the application (owner) account via assumed role
  5. Integration rule DLQ (SQS) in the application account via assumed role
  6. CloudWatch dashboard, alarms, and metrics configuration in the platform account
- **AND THEN** upon successful completion, the workflow SHALL emit a `DomainCreationSucceeded` event with all resource ARNs
- **AND THEN** the control plane SHALL update the domain record in DynamoDB to Active with the resource ARNs

#### Scenario: Provisioning workflow step failure

- **WHEN** any step in the provisioning workflow fails (e.g., AWS API call fails, assumed role is denied, resource limit reached)
- **THEN** the workflow SHALL emit a `DomainCreationFailed` event with the error code, error message, and the name of the failed step
- **AND THEN** the control plane SHALL update the domain record in DynamoDB to a Failed state with the error details
- **AND THEN** resources created by earlier steps SHALL be retained for diagnosis (not automatically rolled back)

#### Scenario: Provisioning uses namespace convention for resource names

- **WHEN** the provisioning workflow creates domain resources
- **THEN** all resource names SHALL be derived from the configured namespace convention (default: `{env}-{org}-{domain}`)
- **AND THEN** the environment, organization, and domain name values SHALL be sourced from the registration request and control plane configuration

### Requirement: Provisioning workflow lifecycle events

The provisioning workflow SHALL emit lifecycle events to the control plane event bus at key points: when provisioning starts, when it succeeds, and when it fails. These events SHALL conform to the event types and minimum payloads defined in the domain-management spec.

#### Scenario: Provisioning started event

- **WHEN** the provisioning workflow begins execution
- **THEN** it SHALL emit a `DomainCreationStarted` event to the control plane event bus with: requestId, domainId, workflowExecutionId, and startedAt

#### Scenario: Provisioning succeeded event

- **WHEN** the provisioning workflow completes all steps successfully
- **THEN** it SHALL emit a `DomainCreationSucceeded` event with: requestId, domainId, workflowExecutionId, completedAt, and resourceArns (map of all created resource ARNs)

#### Scenario: Provisioning failed event

- **WHEN** the provisioning workflow fails at any step
- **THEN** it SHALL emit a `DomainCreationFailed` event with: requestId, domainId, workflowExecutionId, failedAt, errorCode, errorMessage, and failedStep
