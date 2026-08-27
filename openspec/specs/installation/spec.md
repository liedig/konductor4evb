# Capability: Installation

## Purpose

How a first-time operator deploys Konductor into their own AWS environment: a single runnable installation guide covering the three-stack deploy model (notifications, core, trust), prerequisites, smoke-test walkthrough, troubleshooting, and uninstall.

## Requirements

### Requirement: Installation guide

The project SHALL provide a complete, runnable installation guide at `docs/installation.md` that walks a first-time operator through deploying Konductor into their own AWS environment from scratch. The guide SHALL reflect that Konductor is deployed once per organisation into a dedicated platform account, and SHALL NOT suggest deploying Konductor per environment (the environment concept belongs to domains Konductor provisions, not to Konductor itself).

#### Scenario: First-time operator completes installation

- **WHEN** a first-time operator follows `docs/installation.md` from start to finish with one platform account and one domain account
- **THEN** they SHALL have the notifications stack, core stack, and trust stack deployed
- **AND THEN** they SHALL be able to register a domain, approve it, and verify it reaches ACTIVE status

#### Scenario: Installation parameters do not include Environment for Konductor stacks

- **WHEN** the operator deploys the notifications stack, core stack, or trust stack
- **THEN** the documented parameter tables SHALL NOT include an `Environment` parameter
- **AND THEN** the guide SHALL explain that `Environment` is a domain-level concept applied when a domain is registered, not a platform-level concept

#### Scenario: README points to installation guide

- **WHEN** a reader views the README
- **THEN** the "Getting Started" section SHALL contain a pointer to `docs/installation.md`
- **AND THEN** the README SHALL NOT contain duplicated installation steps

### Requirement: Smoke test walkthrough

The installation guide SHALL include a smoke-test section that exercises the core domain lifecycle: register a domain, approve it, and verify ACTIVE status with provisioned resources. The smoke test SHALL use the domain name as the domain identifier throughout — the consumer provides the domain name at registration time and uses it directly for subsequent API calls without needing to discover a surrogate identifier. The smoke test SHALL use distinct email addresses for the domain owner and secondary owner, different from the platform team email, to accurately reflect the separation of concerns between domain owners and platform operators.

#### Scenario: Smoke test covers domain lifecycle

- **WHEN** the operator follows the smoke-test section
- **THEN** the guide SHALL provide commands for registering a domain via the API using the domain name in the request body with at least one secondary owner email
- **AND THEN** the guide SHALL provide commands for approving the domain via the API using the task token
- **AND THEN** the guide SHALL describe how to verify the domain reached ACTIVE status by calling `GET /domains/{domainName}`
- **AND THEN** the guide SHALL describe how to verify that resources were provisioned in both accounts
- **AND THEN** the smoke test SHALL NOT require polling DynamoDB to discover a domain identifier
- **AND THEN** the smoke test SHALL use a primary owner email distinct from the platform team email (e.g., `sliedig+{account}+primary@example.com` for the owner and `sliedig+{account}+secondary@example.com` for the secondary owner)

### Requirement: Troubleshooting section

The installation guide SHALL include a troubleshooting section covering common failure modes encountered during first-time installation.

#### Scenario: Troubleshooting covers common failures

- **WHEN** an operator encounters a failure during installation
- **THEN** the guide SHALL cover SES sandbox verification issues
- **AND THEN** the guide SHALL cover RAM share failures (missing Organization)
- **AND THEN** the guide SHALL cover trust role AssumeRole denied errors
- **AND THEN** the guide SHALL cover SAM build cache staleness

### Requirement: Uninstall instructions

The installation guide SHALL include an uninstall section that walks the operator through removing all deployed resources.

#### Scenario: Operator removes Konductor installation

- **WHEN** an operator follows the uninstall section
- **THEN** the guide SHALL describe deleting all domains first (waiting for DELETED status)
- **AND THEN** the guide SHALL describe deleting the core stack, trust stacks, and notifications stack
- **AND THEN** the guide SHALL note that DynamoDB tables and CloudWatch log groups are retained on stack delete and must be manually cleaned up

### Requirement: Makefile trust stack deployment target

The project SHALL provide a `deploy-trust` Makefile target that deploys `trust-stack.yaml` to a domain account using `aws cloudformation deploy`. The target SHALL accept `APP_PROFILE` (required, the AWS CLI profile for the target domain account) and `ORGANIZATION` (optional, defaults to `acme`) as make variables. The target SHALL automatically resolve `KonductorFunctionRoleArn` and `ProvisioningWorkflowRoleArn` from the deployed `konductor-core` stack outputs using the platform account credentials, eliminating manual copy-paste of role ARNs. The target SHALL use `--capabilities CAPABILITY_NAMED_IAM` and tag the stack with `Application=konductor Component=trust`.

#### Scenario: Operator deploys trust stack with valid profile

- **WHEN** the operator runs `make deploy-trust APP_PROFILE=demo` after the core stack is deployed
- **THEN** the target SHALL fetch `KonductorFunctionRoleArn` and `ProvisioningWorkflowRoleArn` from the `konductor-core` stack outputs
- **AND THEN** the target SHALL deploy `trust-stack.yaml` to the domain account identified by `APP_PROFILE`
- **AND THEN** the deployed stack SHALL be named `konductor-trust`
- **AND THEN** the stack SHALL have tags `Application=konductor` and `Component=trust`

#### Scenario: Operator deploys trust stack with custom organization

- **WHEN** the operator runs `make deploy-trust APP_PROFILE=demo ORGANIZATION=myorg`
- **THEN** the `Organization` parameter passed to the trust stack SHALL be `myorg`
- **AND THEN** the trust role created SHALL be named `myorg-konductor-trust-role`

#### Scenario: Operator runs deploy-trust without APP_PROFILE

- **WHEN** the operator runs `make deploy-trust` without setting `APP_PROFILE`
- **THEN** the target SHALL fail with an error message indicating `APP_PROFILE` is required

#### Scenario: Operator runs deploy-trust before core stack is deployed

- **WHEN** the operator runs `make deploy-trust APP_PROFILE=demo` and the `konductor-core` stack does not exist
- **THEN** the target SHALL fail when attempting to fetch stack outputs
- **AND THEN** the error SHALL indicate the `konductor-core` stack was not found

### Requirement: Makefile trust stack teardown target

The project SHALL provide a `teardown-trust` Makefile target that deletes the `konductor-trust` stack from a domain account. The target SHALL accept `APP_PROFILE` (required) as a make variable and use `AWS_REGION` for the region.

#### Scenario: Operator removes trust stack from domain account

- **WHEN** the operator runs `make teardown-trust APP_PROFILE=demo`
- **THEN** the target SHALL delete the `konductor-trust` stack from the domain account identified by `APP_PROFILE`

#### Scenario: Operator runs teardown-trust without APP_PROFILE

- **WHEN** the operator runs `make teardown-trust` without setting `APP_PROFILE`
- **THEN** the target SHALL fail with an error message indicating `APP_PROFILE` is required

### Requirement: Trust stack deployment documentation references Makefile

The trust stack deployment guide at `docs/trust-stack-deployment.md` SHALL reference `make deploy-trust` as the primary deployment method, while retaining the manual `aws cloudformation deploy` command as an alternative for advanced use cases.

#### Scenario: Documentation shows Makefile target as primary method

- **WHEN** a reader views `docs/trust-stack-deployment.md`
- **THEN** the guide SHALL present `make deploy-trust APP_PROFILE=<profile>` as the primary deployment command
- **AND THEN** the guide SHALL retain the manual `aws cloudformation deploy` command as an alternative
