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

### Requirement: Multi-environment provisioning coordinator

Domain provisioning SHALL be coordinated by an outer Step Functions workflow (`provision-domain`) that orchestrates per-environment provisioning via an embedded inner workflow (`provision-env`). The outer workflow SHALL run per-environment provisioning in parallel using a Map state. The outer workflow SHALL enforce an all-or-nothing contract across environments: if any environment fails to provision, the domain SHALL be marked `PROVISIONING_FAILED` and all environments that provisioned successfully SHALL be torn down via the `deprovision-env` workflow. The domain namespace SSM parameter and RAM resource share SHALL be managed by the outer workflow at the domain level, above the Map state, and SHALL NOT be rolled back on provisioning failure.

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

#### Scenario: Namespace and RAM share persist across retries

- **WHEN** the outer provisioning workflow fails after the SSM namespace parameter and RAM share have already been created
- **THEN** the namespace parameter SHALL remain in place
- **AND THEN** the RAM share association SHALL remain in place
- **AND THEN** a subsequent retry of provisioning for the same domain SHALL reuse the existing namespace and RAM share as idempotent no-ops

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

### Requirement: Per-environment deprovisioning workflow

A standalone Step Functions workflow (`deprovision-env`) SHALL idempotently delete one environment's worth of domain resources. It SHALL accept the per-env resource ARN set and the `ownerAccountId` as inputs. It SHALL be reused by (a) the outer provisioning workflow's all-or-nothing rollback path, and (b) future domain deletion flows. Each deletion step SHALL tolerate "resource already gone" responses as successful no-ops. Failures SHALL be collected into a `compensationErrors` output array; the workflow SHALL NOT halt on individual step failure.

#### Scenario: Deprovisioning deletes all resources for one environment

- **WHEN** the `deprovision-env` workflow is invoked with a complete per-env resource ARN set
- **THEN** it SHALL delete integration DLQ, app event bus (in owner account via assumed role), subscription DLQ, schema registry, platform event bus, dashboard, and alarms for that environment
- **AND THEN** each successful deletion SHALL advance to the next step
- **AND THEN** each failed deletion SHALL be appended to `compensationErrors` with the resource identifier and error detail

#### Scenario: Deprovisioning is idempotent

- **WHEN** the `deprovision-env` workflow is invoked against resources that have already been partially or fully deleted
- **THEN** "resource not found" errors from AWS APIs SHALL be treated as successful no-ops
- **AND THEN** the workflow SHALL continue to the next step

### Requirement: Provisioning workflow lifecycle events

The provisioning workflow SHALL emit lifecycle events to the Nexus event bus for provisioning outcomes only. No intermediate events SHALL be emitted (neither per-step within an env nor per-env completion). The outer workflow SHALL be the sole emitter of `DomainProvisioningSucceeded` and `DomainProvisioningFailed`. The event source SHALL be `nexus.domain-management`. Event payloads SHALL identify the domain by `domainName` and SHALL NOT include a `domainId` field.

#### Scenario: Provisioning succeeded event

- **WHEN** the outer provisioning workflow completes all environments successfully
- **THEN** it SHALL emit a `DomainProvisioningSucceeded` event with source `nexus.domain-management`, `domainName`, `workflowExecutionId`, `completedAt`, `environments`, `resourceArns` (map keyed by environment), `namespaceParameter` (SSM parameter path), and `isNewAccount` flag

#### Scenario: Provisioning failed event

- **WHEN** the outer provisioning workflow fails at any environment
- **THEN** it SHALL emit a `DomainProvisioningFailed` event with source `nexus.domain-management`, `domainName`, `workflowExecutionId`, `failedAt`, `failedEnvironment` (the first environment observed to fail), `failedStep` (step within that environment), `errorCode`, `errorMessage`, and `compensationErrors` (aggregate across inner and outer compensation)

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

### Requirement: Provisioning uses platform-layer names for Nexus-internal resources

The provisioning workflow SHALL reference Nexus platform resources (the Nexus event bus, execution roles, log groups, DynamoDB tables) using the fixed `{org}-nexus-*` pattern with no environment segment. Domain-resource names (per-env buses, registries, DLQs, dashboards, alarms) are unaffected by this change — they continue to follow the `{org}-{env}-{domain}-*` pattern established by `multi-env-domain-provisioning`.

#### Scenario: Provisioning emits events on the Nexus platform bus

- **WHEN** the provisioning workflow emits `DomainProvisioningSucceeded` or `DomainProvisioningFailed` events
- **THEN** the target event bus SHALL be `{org}-nexus-bus`

#### Scenario: Provisioning workflow definition substitutions exclude `Environment`

- **WHEN** the provisioning workflow is built from its ASL definition
- **THEN** the `DefinitionSubstitutions` block SHALL NOT include an `Environment` key
- **AND THEN** no substitution in the state machine SHALL reference `${Environment}` — env is sourced from the domain record per Map iteration, not from a deploy-time SAM substitution

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
