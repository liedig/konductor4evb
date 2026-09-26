# Spec Delta

## MODIFIED Requirements

### Requirement: Domain request and owner management

The console SHALL provide a domain-request form collecting domain name, owning team (picked from the caller's team groups; pre-selected when exactly one), owner account ID, owner email, at least one secondary owner email, optional description and an optional cost center. The form SHALL also offer one optional account-override field per platform-configured environment (read from the runtime configuration's environment list): an empty field means the owner account hosts that environment, and a filled field pins that environment to the given account. Client validation SHALL mirror the API contract (name pattern, 12-digit account, email formats, min one secondary owner, cost center non-empty when supplied and at most 256 characters, each account override either empty or a 12-digit account ID). The submitted request SHALL carry only the overridden environments as a sparse `environmentAccounts` map, omitting the field entirely when every environment uses the owner account. Owners SHALL be able to edit description, owner email, secondary owner emails, and cost center from the domain detail page; all other attributes SHALL render read-only, including the effective environment → account map when the API provides it. Console links to per-environment domain resources SHALL resolve against the environment's effective account rather than the owner account. Platform admins SHALL be able to disable and re-enable domains from the admin domain list.

#### Scenario: Request submits as an async command

- **WHEN** an owner submits a valid request
- **THEN** the console SHALL POST the registration (including `owningTeam`) and show the accepted command with its request ID
- **AND THEN** the new domain SHALL appear in the caller's list as pending

#### Scenario: Override fields render per configured environment

- **WHEN** an owner opens the domain-request form on a platform configured with environments `dev`, `staging`, and `prod`
- **THEN** the form SHALL show one optional account field per environment
- **AND THEN** every field SHALL present empty as "use the owner account"

#### Scenario: Sparse overrides submit

- **WHEN** an owner fills only the `prod` override with a 12-digit account ID and submits
- **THEN** the request body SHALL carry `environmentAccounts: {"prod": "…"}` and no keys for untouched environments
- **AND THEN** a request with every override empty SHALL omit `environmentAccounts` entirely

#### Scenario: Malformed override is caught before submission

- **WHEN** an owner enters a value that is not a 12-digit account ID into an override field and submits
- **THEN** the console SHALL label the offending field with the format error and send no request

#### Scenario: Domain detail shows the effective account map

- **WHEN** the describe response carries `environmentAccounts`
- **THEN** the domain detail page SHALL render the effective environment → account map read-only
- **AND THEN** console links for an environment's domain-account resources SHALL target the environment's resolved account, so resources in an override account remain linked

#### Scenario: Owner edits only the mutable fields

- **WHEN** an owner saves changes from the domain detail edit form
- **THEN** the console SHALL submit only description, emails, and cost center
- **AND THEN** name, account, environments, and the account mapping SHALL have no editable affordance

#### Scenario: Cost center edit is pre-filled and validated

- **WHEN** an owner opens the domain detail edit form
- **THEN** the cost center field SHALL be pre-filled with the current value
- **AND THEN** an empty cost center SHALL be rejected client-side as the API contract requires

#### Scenario: Platform admin disables a domain

- **WHEN** a platform admin chooses disable on an ACTIVE domain and confirms
- **THEN** the console SHALL call the state endpoint and reflect INACTIVE
- **AND THEN** non-admin users SHALL never see the affordance

### Requirement: Approvals inbox

Platform admins SHALL review pending registrations from a dedicated inbox showing request details (name, description, owning team, owner account and emails, age). When the pending registration carries the resolved `environmentAccounts` map, the card SHALL show the effective account per environment so the approver sees where resources would land; registrations predating the field SHALL render without the map. Approve SHALL record the decision immediately; reject SHALL require a justification. Decided items SHALL leave the inbox on success, and failures (expired token, conflict) SHALL surface a descriptive error.

#### Scenario: Approve from the inbox

- **WHEN** a platform admin approves a pending registration
- **THEN** the console SHALL submit the decision with the registration's task token
- **AND THEN** the item SHALL be removed from the inbox

#### Scenario: Approval card shows the resolved account map

- **WHEN** a pending registration resolves `dev` and `staging` to the owner account and `prod` to another account
- **THEN** the approval card SHALL list all three environment → account pairs
- **AND THEN** a registration without the map SHALL render the remaining request details unchanged

#### Scenario: Rejection requires justification

- **WHEN** a platform admin attempts to reject without entering a justification
- **THEN** the console SHALL block submission and mark the justification field as required
