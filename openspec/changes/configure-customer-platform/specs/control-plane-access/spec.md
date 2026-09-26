## ADDED Requirements

### Requirement: Platform access administration stays customer-controlled

Global-role group membership SHALL be administered by the customer operator through Cognito administrative actions. Konductor SHALL NOT provide API routes, console features, setup tooling, or configuration settings that create users or modify membership of the controlled global-role groups. No Konductor-authorized path SHALL allow a caller to grant platform global roles to itself or to others; resource-role bindings and entitlements SHALL NOT expand into global roles. The OAuth authorizer's resolution of `platform-admin` SHALL remain independent of grant storage, so an operator with Cognito administrative access can always establish or recover the first platform administrator.

#### Scenario: No self-escalation path exists

- **WHEN** an authenticated caller without a platform global role attempts to obtain one through any Konductor API, binding, entitlement, or configuration mechanism
- **THEN** no such mechanism SHALL exist
- **AND THEN** the attempt SHALL fail without modifying any group membership or global-role state

#### Scenario: Operator establishes the first platform administrator

- **WHEN** an operator with Cognito administrative access adds a user to the controlled `platform-admin` group on a fresh installation
- **THEN** that user's next issued token SHALL resolve the `platform-admin` global role
- **AND THEN** this SHALL NOT depend on any existing Konductor principal, binding, or entitlement
