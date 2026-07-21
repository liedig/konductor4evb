# Capability: Domain Provisioning Workflow (delta)

## MODIFIED Requirements

### Requirement: Per-environment provisioning inner workflow

An inner Step Functions workflow (`provision-env`) SHALL encapsulate the creation of one environment's worth of domain resources. It SHALL accept `domainName`, `ownerAccountId`, and `environment` as inputs. On success it SHALL return the per-environment resource ARNs. On failure it SHALL run its own saga compensation to delete any resources it created before the failure, and then surface the failure to the outer workflow.

The inner workflow's resource creation sequence SHALL match the existing per-env sequence with one addition after app event bus creation: create platform event bus, set PutEvent policy, create schema registry, create subscription DLQ, assume the trust role in the owner account, create app event bus, **attach the platform delivery resource policy to the app event bus** (permitting `events:PutEvents` from the platform account, via the trust role's `events:PutPermission`), create integration DLQ, create dashboard, create alarms. Resource names SHALL be env-scoped using the configurable namespace convention defaulting to `{org}-{env}-{domain}-<suffix>`.

#### Scenario: Inner workflow creates one environment's resources

- **WHEN** the inner workflow receives an input with `environment: "dev"` and the domain's metadata (including `domainName`)
- **THEN** it SHALL create all per-env resources in the documented order, using `dev` as the env segment in every resource name
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
