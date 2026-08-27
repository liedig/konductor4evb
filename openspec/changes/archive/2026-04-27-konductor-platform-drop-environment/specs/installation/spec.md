## MODIFIED Requirements

### Requirement: Installation guide

The project SHALL provide a complete, runnable installation guide at `docs/installation.md` that walks a first-time operator through deploying Konductor into their own AWS environment from scratch. The guide SHALL reflect that Konductor is deployed once per organisation into a dedicated platform account, and SHALL NOT suggest deploying Konductor per environment (the environment concept belongs to domains Konductor provisions, not to Konductor itself).

#### Scenario: First-time operator completes installation

- **WHEN** a first-time operator follows `docs/installation.md` from start to finish with one platform account and one application account
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
