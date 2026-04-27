# Capability: Installation

## Purpose
TBD

## Requirements

### Requirement: Installation guide

The project SHALL provide a complete, runnable installation guide at `docs/installation.md` that walks a first-time operator through deploying Nexus into their own AWS environment from scratch. The guide SHALL reflect that Nexus is deployed once per organisation into a dedicated platform account, and SHALL NOT suggest deploying Nexus per environment (the environment concept belongs to domains Nexus provisions, not to Nexus itself).

#### Scenario: First-time operator completes installation

- **WHEN** a first-time operator follows `docs/installation.md` from start to finish with one platform account and one application account
- **THEN** they SHALL have the notifications stack, core stack, and trust stack deployed
- **AND THEN** they SHALL be able to register a domain, approve it, and verify it reaches ACTIVE status

#### Scenario: Installation parameters do not include Environment for Nexus stacks

- **WHEN** the operator deploys the notifications stack, core stack, or trust stack
- **THEN** the documented parameter tables SHALL NOT include an `Environment` parameter
- **AND THEN** the guide SHALL explain that `Environment` is a domain-level concept applied when a domain is registered, not a platform-level concept

#### Scenario: README points to installation guide

- **WHEN** a reader views the README
- **THEN** the "Getting Started" section SHALL contain a pointer to `docs/installation.md`
- **AND THEN** the README SHALL NOT contain duplicated installation steps

### Requirement: Smoke test walkthrough

The installation guide SHALL include a smoke-test section that exercises the core domain lifecycle: register a domain, approve it, and verify ACTIVE status with provisioned resources.

#### Scenario: Smoke test covers domain lifecycle

- **WHEN** the operator follows the smoke-test section
- **THEN** the guide SHALL provide commands for registering a domain via the API
- **AND THEN** the guide SHALL provide commands for approving the domain via the API
- **AND THEN** the guide SHALL describe how to verify the domain reached ACTIVE status and that resources were provisioned in both accounts

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

#### Scenario: Operator removes Nexus installation

- **WHEN** an operator follows the uninstall section
- **THEN** the guide SHALL describe deleting all domains first (waiting for DELETED status)
- **AND THEN** the guide SHALL describe deleting the core stack, trust stacks, and notifications stack
- **AND THEN** the guide SHALL note that DynamoDB tables and CloudWatch log groups are retained on stack delete and must be manually cleaned up
