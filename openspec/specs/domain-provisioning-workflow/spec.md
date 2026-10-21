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

### Requirement: Per-environment provisioning inner workflow

An inner Step Functions workflow (`provision-env`) SHALL encapsulate the creation of one environment's worth of domain resources. It SHALL accept `domainName`, `environment`, and the account resolved for that environment as inputs. On success it SHALL return the per-environment resource ARNs. On failure it SHALL run its own saga compensation to delete any resources it created before the failure, and then surface the failure to the outer workflow.

The inner workflow's resource creation sequence SHALL match the existing per-env sequence with one addition after app event bus creation: create platform event bus, set PutEvent policy, create schema registry, create subscription DLQ, assume the trust role in the resolved account, create app event bus, **attach the platform delivery resource policy to the app event bus** (permitting `events:PutEvents` from the platform account, via the trust role's `events:PutPermission`), create integration DLQ, create dashboard, create alarms. Resource names SHALL be env-scoped using the configurable namespace convention defaulting to `{org}-{env}-{domain}-<suffix>`.

#### Scenario: Inner workflow creates one environment's resources

- **WHEN** the inner workflow receives an input with `environment: "dev"` and the domain's metadata (including `domainName`)
- **THEN** it SHALL create all per-env resources in the documented order, using `dev` as the env segment in every resource name and the resolved account for every cross-account call
- **AND THEN** the app event bus SHALL carry the platform delivery resource policy before the workflow reports success
- **AND THEN** it SHALL return the resource ARNs keyed by resource type

#### Scenario: Inner workflow fails mid-sequence

- **WHEN** any resource creation step in the inner workflow fails
- **THEN** the inner workflow SHALL execute its saga compensation chain to delete all resources it successfully created in this environment, in reverse order
- **AND THEN** the inner workflow SHALL exit with a failure state that carries the failed step, error code, error message, and any compensation errors
- **AND THEN** the outer workflow SHALL observe this failure via the `.sync:2` integration and trigger the all-or-nothing rollback path

#### Scenario: Delivery policy failure compensates the app bus

- **WHEN** the platform-delivery policy attachment fails after the app event bus was created
- **THEN** compensation SHALL delete the app event bus along with the previously created platform-side resources for this environment

> Note: a "delivery policy backfill for pre-existing domains" requirement was drafted and later descoped. The product is unreleased, so no environment carries domains provisioned before the delivery-policy step; every domain receives the policy at provisioning time.

### Requirement: Per-environment deprovisioning workflow

A standalone Step Functions workflow (`deprovision-env`) SHALL idempotently delete one environment's worth of domain resources. It SHALL accept the per-env resource ARN set and the account resolved for that environment as inputs. It SHALL be reused by (a) the outer provisioning workflow's all-or-nothing rollback path, and (b) future domain deletion flows. Each deletion step SHALL tolerate "resource already gone" responses as successful no-ops, distinguished from genuine failures via service-specific not-found error handling. Steps whose resource identifier is absent from the input SHALL be skipped as successful no-ops. Genuine failures SHALL be collected into a `compensationErrors` output array; the workflow SHALL NOT halt on individual step failure. A non-empty `compensationErrors` array SHALL therefore indicate that resources were actually left behind.

#### Scenario: Deprovisioning deletes all resources for one environment

- **WHEN** the `deprovision-env` workflow is invoked with a complete per-env resource ARN set
- **THEN** it SHALL delete integration DLQ, app event bus (in the resolved account via assumed role), subscription DLQ, schema registry, platform event bus, dashboard, and alarms for that environment
- **AND THEN** each successful deletion SHALL advance to the next step
- **AND THEN** each failed deletion SHALL be appended to `compensationErrors` with the resource identifier and error detail

#### Scenario: Deprovisioning is idempotent

- **WHEN** the `deprovision-env` workflow is invoked against resources that have already been partially or fully deleted
- **THEN** "resource not found" errors from AWS APIs SHALL be treated as successful no-ops
- **AND THEN** the workflow SHALL continue to the next step
- **AND THEN** no entry SHALL be appended to `compensationErrors` for a not-found response

#### Scenario: Partial resource ARN set

- **WHEN** the `deprovision-env` workflow is invoked with an input whose `resourceArns` set lacks identifiers for some steps (for example a domain rejected before provisioning, or a saga that failed before creating them)
- **THEN** steps whose resource identifier is absent SHALL be skipped as successful no-ops
- **AND THEN** no entry SHALL be appended to `compensationErrors` for a skipped step

#### Scenario: Genuine deletion failure is still reported

- **WHEN** a deletion step fails for a reason other than the resource not existing (for example access denied or a service error after retries)
- **THEN** the failure SHALL be appended to `compensationErrors` with the resource identifier and error detail
- **AND THEN** the workflow SHALL continue to the next step

### Requirement: Provisioning workflow lifecycle events

The provisioning workflow SHALL emit lifecycle events to the Konductor event bus for provisioning outcomes only. No intermediate events SHALL be emitted (neither per-step within an env nor per-env completion). The outer workflow SHALL be the sole emitter of `DomainProvisioningSucceeded` and `DomainProvisioningFailed`. The event source SHALL be `konductor.domain-management`. Event payloads SHALL identify the domain by `domainName` and SHALL NOT include a `domainId` field.

#### Scenario: Provisioning succeeded event

- **WHEN** the outer provisioning workflow completes all environments successfully
- **THEN** it SHALL emit a `DomainProvisioningSucceeded` event with source `konductor.domain-management`, `domainName`, `workflowExecutionId`, `completedAt`, `environments`, `resourceArns` (map keyed by environment), `namespaceParameter` (SSM parameter path), and `isNewAccount` flag

#### Scenario: Provisioning failed event

- **WHEN** the outer provisioning workflow fails at any environment
- **THEN** it SHALL emit a `DomainProvisioningFailed` event with source `konductor.domain-management`, `domainName`, `workflowExecutionId`, `failedAt`, `failedEnvironment` (the first environment observed to fail), `failedStep` (step within that environment), `errorCode`, `errorMessage`, and `compensationErrors` (aggregate across inner and outer compensation)

### Requirement: New account detection

The provisioning workflow SHALL detect whether the domain's `ownerAccountId` has been previously seen by querying the DynamoDB `by-owner` GSI. If no other domains with status `ACTIVE` exist for that account, the account SHALL be flagged as new. The new-account flag SHALL be included in the provisioning workflow's input context and surfaced in the approval notification email. Detection is defined on the default account only: accounts that appear solely as `environmentAccounts` overrides are not tracked by the `by-owner` GSI and SHALL NOT affect the flag. Independently of the flag, the approval notification SHALL present the effective environment-to-account map so the approver reviews every account that will receive resources.

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

#### Scenario: Approval notification presents the effective account map

- **WHEN** an approval request notification is rendered for a domain whose effective map resolves any environment to an account other than `ownerAccountId`
- **THEN** the notification SHALL list every environment with its resolved account
- **AND THEN** the approver can see exactly which accounts will receive resources before deciding

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

### Requirement: Provisioning uses platform-layer names for Konductor-internal resources

The provisioning workflow SHALL reference Konductor platform resources (the Konductor event bus, execution roles, log groups, DynamoDB tables) using the fixed `{org}-konductor-*` pattern with no environment segment. Domain-resource names (per-env buses, registries, DLQs, dashboards, alarms) are unaffected by this change — they continue to follow the `{org}-{env}-{domain}-*` pattern established by `multi-env-domain-provisioning`.

#### Scenario: Provisioning emits events on the Konductor platform bus

- **WHEN** the provisioning workflow emits `DomainProvisioningSucceeded` or `DomainProvisioningFailed` events
- **THEN** the target event bus SHALL be `{org}-konductor-bus`

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

### Requirement: Tagging at provisioning

Provisioning SHALL tag every resource it creates with the domain's effective tag set. Per-environment resources (platform event bus, app event bus, schema registry, subscription DLQ, integration DLQ, dashboard, alarms) SHALL carry `konductor:managed-by`, `konductor:domain`, `konductor:environment`, and `konductor:namespace`, plus `cost-center` when the domain record carries one. The domain-level namespace SSM parameter SHALL carry the same set minus `konductor:environment`. Tags SHALL be supplied at resource creation wherever the service supports it; tag application SHALL be part of the saga's tracked steps so a tagging failure fails (and compensates) the environment exactly like any other resource failure.

#### Scenario: Provisioned bus carries the effective tags

- **WHEN** a domain with `cost-center=cc-1234` is provisioned into `dev`
- **THEN** `acme-dev-<domain>-event-bus` in both accounts SHALL be tagged `konductor:managed-by=konductor`, `konductor:domain=<domain>`, `konductor:environment=dev`, `konductor:namespace=<org>-<domain>`, and `cost-center=cc-1234`

#### Scenario: Namespace parameter tagged without environment

- **WHEN** the domain-level namespace parameter is created
- **THEN** it SHALL carry `konductor:managed-by`, `konductor:domain`, and `konductor:namespace`, and SHALL NOT carry `konductor:environment`

#### Scenario: Tag failure fails the environment

- **WHEN** a tagging call fails during provisioning of an environment
- **THEN** that environment SHALL fail and compensate exactly as for any other resource-creation failure

### Requirement: Provisioned-resource retagging on cost-center change

When a committed domain update changes `costCenter`, the platform SHALL converge the `cost-center` tag on every provisioned resource of that domain to the new value. An EventBridge rule on the control plane bus SHALL trigger a dedicated retag workflow for `UpdateDomainSucceeded` events whose `changedFields` contain `costCenter`; all other update events SHALL be ignored by the rule. The workflow SHALL retag every resource recorded in the domain record's per-environment `resourceArns` map (event buses, schema registries, DLQs, dashboards, and alarms) and the domain-level namespace parameter, applying the standard platform tag set with the new `cost-center` value. Resources in the owner's account SHALL be retagged through the `konductor-trust-role`; platform-account resources SHALL be retagged directly. The workflow SHALL be defined in YAML with JSONata as the query language, SHALL tolerate event redelivery by relying on the upsert semantics of the tagging APIs (a duplicate execution re-applies the same tag set), and SHALL emit a failure event on the control plane bus when retagging cannot be completed. Retag failure SHALL NOT roll back the committed domain record update; the record remains the system of record for the intended tag state.

#### Scenario: Cost-center change triggers retagging

- **WHEN** an `UpdateDomainSucceeded` event with `costCenter` in `changedFields` is published to the control plane bus
- **THEN** the retag workflow SHALL start
- **AND THEN** the workflow SHALL retag each resource in the domain's per-environment `resourceArns` map and the namespace parameter with the new `cost-center` value

#### Scenario: Non-cost-center updates do not trigger retagging

- **WHEN** an `UpdateDomainSucceeded` event is published whose `changedFields` do not contain `costCenter`
- **THEN** the retag workflow SHALL NOT start

#### Scenario: Redelivered event converges to the same tag state

- **WHEN** the same `UpdateDomainSucceeded` event (same domain and `requestId`) is delivered more than once
- **THEN** each delivery MAY start a workflow execution (rule targets cannot set execution names, so executions are auto-named)
- **AND THEN** every execution SHALL re-apply the same tag values, because the tagging APIs used are upserts

#### Scenario: Owner-account resources are retagged cross-account

- **WHEN** the retag workflow processes an environment
- **THEN** resources recorded in the owner's account SHALL be retagged using the `konductor-trust-role` in that account
- **AND THEN** platform-account resources SHALL be retagged without assuming the trust role

#### Scenario: Retag failure is surfaced

- **WHEN** the retag workflow cannot retag a resource (for example, a missing trust role or throttling that exhausts retries)
- **THEN** the workflow SHALL emit a failure event on the control plane bus identifying the domain and the failed step
- **AND THEN** the domain record's committed `costCenter` value SHALL remain unchanged by the failure

### Requirement: Per-environment account resolution

The outer provisioning workflow SHALL resolve the target account for each environment from the approved event's effective `environmentAccounts` map before invoking the inner workflow, and SHALL pass the resolved account — never the domain's default `ownerAccountId` unconditionally — to every per-environment step, including saga compensation and rollback. Resolution SHALL be total: every environment in the domain's stamped environment list SHALL resolve to exactly one account. Workflow steps that span accounts (cost-center retagging) SHALL iterate environments and resolve the trust-role credentials per environment from the domain record's effective map; because every tag target is an environment-scoped resource, a shared account is never processed twice for the same resource — the account only scopes the credentials.

#### Scenario: Single-account domain resolves uniformly

- **WHEN** the approved event's effective map resolves every environment to the default `ownerAccountId`
- **THEN** the workflow SHALL pass that account to every per-environment invocation
- **AND THEN** every cross-account step, including retagging, SHALL assume the trust role in that account

#### Scenario: Overridden environment provisions into its mapped account

- **WHEN** the effective map resolves `prod` to a different account than `dev` and `staging`
- **THEN** the inner workflow invocation for `prod` SHALL receive the mapped account and SHALL assume the trust role in that account
- **AND THEN** the invocations for `dev` and `staging` SHALL receive the default account

#### Scenario: Shared account never causes double-processing

- **WHEN** two or more environments resolve to the same account
- **THEN** each environment's resources SHALL still be processed in that environment's own iteration under the shared account's credentials
- **AND THEN** no environment-scoped resource SHALL be provisioned, tagged, or rolled back twice

### Requirement: Domain-level system-events topic provisioning

Provisioning SHALL create and configure one system-events topic at domain scope before marking the domain `ACTIVE`. The topic policy SHALL use the registered owner and effective environment accounts and the system-event protocol restrictions. Its ARN SHALL be persisted as top-level `systemEventsTopicArn`, outside per-environment `resourceArns`.

#### Scenario: All environments share the provisioned topic
- **WHEN** a domain with several environments completes provisioning
- **THEN** topic creation and policy configuration SHALL have succeeded once at domain scope
- **AND** the domain record SHALL contain its topic ARN independently of each environment's resource map

#### Scenario: Repeated provisioning converges on the same managed topic
- **WHEN** the same provisioning execution retries topic creation or policy application
- **THEN** it SHALL reuse the deterministically named, verified platform-managed topic
- **AND** it SHALL converge on the same policy without creating another topic

#### Scenario: Topic name belongs to an unrelated resource
- **WHEN** the expected name is already used by a topic whose ownership cannot be verified
- **THEN** provisioning SHALL fail visibly rather than adopt, retag, repolicy, or delete that resource

### Requirement: Topic provisioning compensation

A new-domain provisioning failure SHALL compensate its system-events topic alongside successful environment resources. Topic deletion SHALL be idempotent, clear any recorded ARN only after successful deletion, and collect genuine errors without preventing the terminal failure status. Namespace and RAM resources SHALL retain their existing retry behavior.

#### Scenario: Policy configuration fails after topic creation
- **WHEN** topic creation succeeds but policy configuration exhausts retries
- **THEN** provisioning SHALL delete the topic it owns and reach `PROVISIONING_FAILED`
- **AND** it SHALL NOT report a ready topic ARN

#### Scenario: Environment failure rolls back the shared topic
- **WHEN** an environment fails after the shared topic was created
- **THEN** rollback SHALL compensate both successful sibling environments and the shared topic
- **AND** a topic deletion error SHALL be included in the existing compensation error reporting

#### Scenario: Repeated compensation finds no topic
- **WHEN** rollback is retried after the topic has already been deleted
- **THEN** absence SHALL count as success, not an additional compensation failure

### Requirement: Non-destructive topic backfill

The platform SHALL provide an operator-run, idempotent backfill for existing `ACTIVE` and `INACTIVE` domains. It SHALL create or reconcile only the domain's managed topic, policy, tags, and recorded ARN. It SHALL preserve lifecycle status and business resources, paginate the domain inventory, and report failures per domain without hiding successful work.

#### Scenario: Existing active domain receives a topic
- **WHEN** the operator runs backfill for an `ACTIVE` domain with no topic ARN
- **THEN** its topic and policy SHALL be established and its ARN recorded
- **AND** its buses, registries, queues, subscriptions, and lifecycle status SHALL remain unchanged

#### Scenario: Inactive domain is not re-enabled
- **WHEN** backfill processes an `INACTIVE` domain
- **THEN** its topic SHALL be established without enabling the domain or any subscription

#### Scenario: Backfill is retried after partial completion
- **WHEN** backfill previously configured a topic but failed before recording its ARN
- **THEN** a retry SHALL verify and reuse the topic, preserve existing subscriptions, and record the ARN

#### Scenario: Inventory spans several pages
- **WHEN** the domain inventory exceeds one database page and one eligible domain fails
- **THEN** backfill SHALL process all pages and report that domain's failure
- **AND** pending, rejected, and provisioning-failed domains SHALL not be treated as provisioned domains

### Requirement: Default owner email subscription

Provisioning SHALL subscribe the domain's owner email address to its system-events topic as the default endpoint, idempotently and without blocking provisioning on the address owner's confirmation. A committed owner-email change SHALL reconcile the default subscription, and the backfill SHALL ensure it for existing domains.

#### Scenario: Owner email is subscribed once during provisioning
- **WHEN** a domain with an owner email completes topic provisioning
- **THEN** the topic SHALL have one email subscription for that address
- **AND** the subscription SHALL remain pending until the owner confirms it, which SHALL NOT fail or hold provisioning

#### Scenario: Provisioning retry does not duplicate the subscription
- **WHEN** the provisioning execution retries after the owner email was already subscribed
- **THEN** it SHALL converge on the existing subscription rather than creating a duplicate

#### Scenario: Owner email change follows the domain update
- **WHEN** a committed domain update replaces the owner email
- **THEN** the old address's default subscription SHALL be unsubscribed and the new address subscribed
- **AND** a reconciliation failure SHALL remain observable without reversing the domain metadata update

#### Scenario: Backfill establishes the owner subscription
- **WHEN** backfill processes an existing domain whose topic lacks the owner email subscription
- **THEN** it SHALL subscribe the current owner email and leave other endpoint subscriptions untouched

### Requirement: System-events topic tags follow domain ownership

Topics SHALL carry `konductor:managed-by`, `konductor:domain`, `konductor:namespace`, and the domain's `cost-center` when configured, without an environment tag. A committed cost-center change SHALL retag the recorded topic through the existing retag path. Missing topic ARNs during rollout SHALL be skipped, not inferred or treated as fatal.

#### Scenario: Topic is tagged at creation
- **WHEN** provisioning or backfill creates a topic for a domain with a cost center
- **THEN** it SHALL apply the domain-level managed tags and cost center
- **AND** tag failure SHALL prevent reporting the topic as ready

#### Scenario: Cost center changes after backfill
- **WHEN** a committed cost-center update triggers retagging for a domain with a recorded topic
- **THEN** retagging SHALL update that topic as well as the existing resources
- **AND** retag failure SHALL remain observable without reversing the domain metadata update

#### Scenario: Domain is disabled after provisioning
- **WHEN** a provisioned domain becomes `INACTIVE`
- **THEN** its topic and SNS subscriptions SHALL remain in place like its other retained infrastructure
