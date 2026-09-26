## ADDED Requirements

### Requirement: Platform configuration contract

The platform SHALL define a configuration contract covering every customer-supplied setting: organisation identifier, AWS Organization ARN, domain environment list, resource namespace pattern, notification sender and operator recipient addresses, access-token validity, and OAuth callback/logout URLs. Each setting SHALL be classified as fixed-at-setup or updateable. The organisation identifier, AWS Organization ARN, domain environment list, and namespace pattern SHALL be fixed at setup. Notification addresses, access-token validity, and OAuth URLs SHALL be updateable by redeployment with new values. The platform SHALL remain a single organisation-level control plane; the environment list SHALL describe domain environments only, not separate platform instances.

Deployment tooling SHALL refuse to apply a changed fixed-at-setup value to an existing installation and SHALL report which setting changed and that a new deployment is required. Tooling SHALL NOT rename, replace, orphan, or delete existing resources as a side effect of such a change.

#### Scenario: Fixed-at-setup change is rejected

- **WHEN** an operator deploys with an organisation identifier, environment list, namespace pattern, or AWS Organization ARN that differs from the deployed stack
- **THEN** deployment tooling SHALL refuse before any resource change
- **AND THEN** the refusal SHALL name the differing setting and state that changing it requires a new deployment

#### Scenario: Updateable setting changes by redeployment

- **WHEN** an operator redeploys with new notification addresses, token validity, or OAuth URLs
- **THEN** the new values SHALL be validated
- **AND THEN** the update SHALL apply without a fixed-at-setup rejection

#### Scenario: Single organisation-level control plane

- **WHEN** the environment list is configured
- **THEN** it SHALL describe the environments approved domains are provisioned into
- **AND THEN** it SHALL NOT create additional platform instances or environment-qualified platform resource names

### Requirement: Pre-deployment validation with actionable feedback

The platform SHALL provide a supported pre-deployment validation step that checks every contract setting and reports actionable errors before any deployment occurs. Validation SHALL cover: environment entry syntax, uniqueness, and count; namespace pattern required placeholders; organisation identifier syntax; AWS Organization ARN format; notification address pairing and SES verification prerequisites; and HTTPS for OAuth URLs outside local development. Deployment through the supported path SHALL NOT proceed when validation fails.

#### Scenario: Invalid environment entry fails before deploy

- **WHEN** the configuration contains an environment entry that violates the documented syntax
- **THEN** validation SHALL fail before any deployment
- **AND THEN** the error SHALL name the offending entry and the syntax rule

#### Scenario: Notifications are explicitly enabled or disabled

- **WHEN** exactly one notification address is configured, or a configured address is not verified in SES
- **THEN** validation SHALL fail with an actionable message
- **WHEN** both notification addresses are empty
- **THEN** validation SHALL pass and report that notifications are disabled

### Requirement: Customer configuration kept out of version control

The supported setup path SHALL keep customer-supplied values in a configuration file excluded from version control, with a committed example describing every setting. Validation and deployment SHALL consume the same file so they cannot read different values. For an existing installation without the file, the deployed stack's parameter values SHALL remain the record until the file is generated from them.

#### Scenario: Fresh setup from the committed example

- **WHEN** a customer copies the example configuration and fills in their values
- **THEN** validation and deployment SHALL read those values from that file
- **AND THEN** no customer value SHALL need to be committed to version control

#### Scenario: Existing installation adopts the configuration file

- **WHEN** an existing installation generates the configuration file from its deployed stack parameters
- **THEN** validation SHALL pass against the deployed stack
- **AND THEN** a subsequent redeployment with the generated file SHALL change no fixed-at-setup value
