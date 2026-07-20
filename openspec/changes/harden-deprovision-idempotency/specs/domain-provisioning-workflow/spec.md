# Capability: Domain Provisioning Workflow (delta)

## MODIFIED Requirements

### Requirement: Per-environment deprovisioning workflow

A standalone Step Functions workflow (`deprovision-env`) SHALL idempotently delete one environment's worth of domain resources. It SHALL accept the per-env resource ARN set and the `ownerAccountId` as inputs. It SHALL be reused by (a) the outer provisioning workflow's all-or-nothing rollback path, and (b) future domain deletion flows. Each deletion step SHALL tolerate "resource already gone" responses as successful no-ops, distinguished from genuine failures via service-specific not-found error handling. Steps whose resource identifier is absent from the input SHALL be skipped as successful no-ops. Genuine failures SHALL be collected into a `compensationErrors` output array; the workflow SHALL NOT halt on individual step failure. A non-empty `compensationErrors` array SHALL therefore indicate that resources were actually left behind.

#### Scenario: Deprovisioning deletes all resources for one environment

- **WHEN** the `deprovision-env` workflow is invoked with a complete per-env resource ARN set
- **THEN** it SHALL delete integration DLQ, app event bus (in owner account via assumed role), subscription DLQ, schema registry, platform event bus, dashboard, and alarms for that environment
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
