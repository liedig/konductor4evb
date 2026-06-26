## ADDED Requirements

### Requirement: Makefile trust stack deployment target

The project SHALL provide a `deploy-trust` Makefile target that deploys `trust-stack.yaml` to an application account using `aws cloudformation deploy`. The target SHALL accept `APP_PROFILE` (required, the AWS CLI profile for the target application account) and `ORGANIZATION` (optional, defaults to `acme`) as make variables. The target SHALL automatically resolve `NexusFunctionRoleArn` and `ProvisioningWorkflowRoleArn` from the deployed `nexus-core` stack outputs using the platform account credentials, eliminating manual copy-paste of role ARNs. The target SHALL use `--capabilities CAPABILITY_NAMED_IAM` and tag the stack with `Application=nexus Component=trust`.

#### Scenario: Operator deploys trust stack with valid profile

- **WHEN** the operator runs `make deploy-trust APP_PROFILE=demo` after the core stack is deployed
- **THEN** the target SHALL fetch `NexusFunctionRoleArn` and `ProvisioningWorkflowRoleArn` from the `nexus-core` stack outputs
- **AND THEN** the target SHALL deploy `trust-stack.yaml` to the application account identified by `APP_PROFILE`
- **AND THEN** the deployed stack SHALL be named `nexus-trust`
- **AND THEN** the stack SHALL have tags `Application=nexus` and `Component=trust`

#### Scenario: Operator deploys trust stack with custom organization

- **WHEN** the operator runs `make deploy-trust APP_PROFILE=demo ORGANIZATION=myorg`
- **THEN** the `Organization` parameter passed to the trust stack SHALL be `myorg`
- **AND THEN** the trust role created SHALL be named `myorg-nexus-trust-role`

#### Scenario: Operator runs deploy-trust without APP_PROFILE

- **WHEN** the operator runs `make deploy-trust` without setting `APP_PROFILE`
- **THEN** the target SHALL fail with an error message indicating `APP_PROFILE` is required

#### Scenario: Operator runs deploy-trust before core stack is deployed

- **WHEN** the operator runs `make deploy-trust APP_PROFILE=demo` and the `nexus-core` stack does not exist
- **THEN** the target SHALL fail when attempting to fetch stack outputs
- **AND THEN** the error SHALL indicate the `nexus-core` stack was not found

### Requirement: Makefile trust stack teardown target

The project SHALL provide a `teardown-trust` Makefile target that deletes the `nexus-trust` stack from an application account. The target SHALL accept `APP_PROFILE` (required) as a make variable and use `AWS_REGION` for the region.

#### Scenario: Operator removes trust stack from application account

- **WHEN** the operator runs `make teardown-trust APP_PROFILE=demo`
- **THEN** the target SHALL delete the `nexus-trust` stack from the application account identified by `APP_PROFILE`

#### Scenario: Operator runs teardown-trust without APP_PROFILE

- **WHEN** the operator runs `make teardown-trust` without setting `APP_PROFILE`
- **THEN** the target SHALL fail with an error message indicating `APP_PROFILE` is required

### Requirement: Trust stack deployment documentation references Makefile

The trust stack deployment guide at `docs/trust-stack-deployment.md` SHALL reference `make deploy-trust` as the primary deployment method, while retaining the manual `aws cloudformation deploy` command as an alternative for advanced use cases.

#### Scenario: Documentation shows Makefile target as primary method

- **WHEN** a reader views `docs/trust-stack-deployment.md`
- **THEN** the guide SHALL present `make deploy-trust APP_PROFILE=<profile>` as the primary deployment command
- **AND THEN** the guide SHALL retain the manual `aws cloudformation deploy` command as an alternative
