## MODIFIED Requirements

### Requirement: Human-in-the-loop approval

The approval workflow SHALL include a human approval step using the Step Functions `.waitForTaskToken` pattern. When a registration request is submitted, the workflow SHALL invoke a notification Lambda that persists the task token and sends an email, then block until a callback is received. The control plane SHALL provide a synchronous API endpoint for approvers to submit their decision along with their identity and justification.

#### Scenario: Approval workflow notifies and waits for human decision

- **WHEN** a new domain registration request triggers the approval workflow
- **THEN** the workflow SHALL enter the `NotifyAndWait` state using the `.waitForTaskToken` resource
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
- **THEN** the decide Lambda SHALL call `sfn:SendTaskFailure` with error `RegistrationRejected` and cause containing `decidedBy` and `justification`
- **AND THEN** the approval workflow SHALL catch the `States.TaskFailed` error
- **AND THEN** the workflow SHALL update the domain record to `REJECTED` status with `decidedBy`, `justification`, and `rejectedAt`
- **AND THEN** the workflow SHALL emit the `DomainRegistrationRejected` event

### Requirement: Per-environment provisioning inner workflow

An inner Step Functions workflow (`provision-env`) SHALL encapsulate the creation of one environment's worth of domain resources. It SHALL accept `domainName`, `ownerAccountId`, and `environment` as inputs. On success it SHALL return the per-environment resource ARNs. On failure it SHALL run its own saga compensation to delete any resources it created before the failure, and then surface the failure to the outer workflow.

The inner workflow's resource creation sequence SHALL match the existing per-env sequence: create platform event bus, set PutEvent policy, create schema registry, create subscription DLQ, assume the trust role in the owner account, create app event bus, create integration DLQ, create dashboard, create alarms. Resource names SHALL be env-scoped using the configurable namespace convention defaulting to `{org}-{env}-{domain}-<suffix>`.

#### Scenario: Inner workflow creates one environment's resources

- **WHEN** the inner workflow receives an input with `environment: "dev"` and the domain's metadata (including `domainName`)
- **THEN** it SHALL create all per-env resources in the documented order, using `dev` as the env segment in every resource name
- **AND THEN** it SHALL return the resource ARNs keyed by resource type

#### Scenario: Inner workflow fails mid-sequence

- **WHEN** any resource creation step in the inner workflow fails
- **THEN** the inner workflow SHALL execute its saga compensation chain to delete all resources it successfully created in this environment, in reverse order
- **AND THEN** the inner workflow SHALL exit with a failure state that carries the failed step, error code, error message, and any compensation errors
- **AND THEN** the outer workflow SHALL observe this failure via the `.sync:2` integration and trigger the all-or-nothing rollback path

### Requirement: Provisioning workflow lifecycle events

The provisioning workflow SHALL emit lifecycle events to the Nexus event bus for provisioning outcomes only. No intermediate events SHALL be emitted (neither per-step within an env nor per-env completion). The outer workflow SHALL be the sole emitter of `DomainProvisioningSucceeded` and `DomainProvisioningFailed`. The event source SHALL be `nexus.domain-management`. Event payloads SHALL identify the domain by `domainName` and SHALL NOT include a `domainId` field.

#### Scenario: Provisioning succeeded event

- **WHEN** the outer provisioning workflow completes all environments successfully
- **THEN** it SHALL emit a `DomainProvisioningSucceeded` event with source `nexus.domain-management`, `domainName`, `workflowExecutionId`, `completedAt`, `environments`, `resourceArns` (map keyed by environment), `namespaceParameter` (SSM parameter path), and `isNewAccount` flag

#### Scenario: Provisioning failed event

- **WHEN** the outer provisioning workflow fails at any environment
- **THEN** it SHALL emit a `DomainProvisioningFailed` event with source `nexus.domain-management`, `domainName`, `workflowExecutionId`, `failedAt`, `failedEnvironment` (the first environment observed to fail), `failedStep` (step within that environment), `errorCode`, `errorMessage`, and `compensationErrors` (aggregate across inner and outer compensation)
