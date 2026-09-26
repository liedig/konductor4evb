# Spec Delta

## ADDED Requirements

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

## MODIFIED Requirements

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
