## ADDED Requirements

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
- **AND THEN** the outer workflow SHALL emit a `DomainProvisioningSucceeded` event with the domain ID, workflow execution ID, completion timestamp, environments list, per-environment resource ARNs map, namespace parameter path, and `isNewAccount` flag

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

An inner Step Functions workflow (`provision-env`) SHALL encapsulate the creation of one environment's worth of domain resources. It SHALL accept `domainId`, `domainName`, `ownerAccountId`, and `environment` as inputs. On success it SHALL return the per-environment resource ARNs. On failure it SHALL run its own saga compensation to delete any resources it created before the failure, and then surface the failure to the outer workflow.

The inner workflow's resource creation sequence SHALL match the existing per-env sequence: create platform event bus, set PutEvent policy, create schema registry, create subscription DLQ, assume the trust role in the owner account, create app event bus, create integration DLQ, create dashboard, create alarms. Resource names SHALL be env-scoped using the configurable namespace convention defaulting to `{org}-{env}-{domain}-<suffix>`.

#### Scenario: Inner workflow creates one environment's resources

- **WHEN** the inner workflow receives an input with `environment: "dev"` and the domain's metadata
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

## REMOVED Requirements

### Requirement: Provisioning workflow resource creation (single-environment)

**Reason**: Replaced by the two-layer design (outer `provision-domain` coordinator + inner `provision-env`). The single-workflow-that-does-everything-for-one-env model is superseded. The inner workflow's resource-creation sequence is preserved verbatim but now runs once per environment inside a Map.

**Migration**: See the `Multi-environment provisioning coordinator` and `Per-environment provisioning inner workflow` requirements above; the old workflow's behaviour is fully preserved within `provision-env` for the single-env case.

### Requirement: Saga rollback on provisioning failure (single-environment)

**Reason**: Split into two layers. Inner saga compensation (within-env partial failure) is preserved in `provision-env`. Cross-env rollback (all-or-nothing) is new behaviour of the outer `provision-domain` workflow and is captured in the `Multi-environment provisioning coordinator` requirement above.

**Migration**: See the outer-workflow scenarios above for the new cross-env semantics; inner-env saga behaviour is unchanged from the current implementation, just relocated into `provision-env.asl.yaml`.

## MODIFIED Requirements

### Requirement: Provisioning workflow lifecycle events

The provisioning workflow SHALL emit lifecycle events to the Nexus event bus for provisioning outcomes only. No intermediate events SHALL be emitted (neither per-step within an env nor per-env completion). The outer workflow SHALL be the sole emitter of `DomainProvisioningSucceeded` and `DomainProvisioningFailed`. The event source SHALL be `nexus.domain-management`.

#### Scenario: Provisioning succeeded event

- **WHEN** the outer provisioning workflow completes all environments successfully
- **THEN** it SHALL emit a `DomainProvisioningSucceeded` event with source `nexus.domain-management`, `domainId`, `workflowExecutionId`, `completedAt`, `environments`, `resourceArns` (map keyed by environment), `namespaceParameter` (SSM parameter path), and `isNewAccount` flag

#### Scenario: Provisioning failed event

- **WHEN** the outer provisioning workflow fails at any environment
- **THEN** it SHALL emit a `DomainProvisioningFailed` event with source `nexus.domain-management`, `domainId`, `workflowExecutionId`, `failedAt`, `failedEnvironment` (the first environment observed to fail), `failedStep` (step within that environment), `errorCode`, `errorMessage`, and `compensationErrors` (aggregate across inner and outer compensation)
