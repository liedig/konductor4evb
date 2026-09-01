# web-ui Specification Delta

## MODIFIED Requirements

### Requirement: Domain request and owner management

The console SHALL provide a domain-request form collecting domain name, owning team (picked from the caller's team groups; pre-selected when exactly one), owner account ID, owner email, at least one secondary owner email, optional description and an optional cost center. Client validation SHALL mirror the API contract (name pattern, 12-digit account, email formats, min one secondary owner, cost center non-empty when supplied and at most 256 characters). Owners SHALL be able to edit description, owner email, secondary owner emails, and cost center from the domain detail page; all other attributes SHALL render read-only. Platform admins SHALL be able to disable and re-enable domains from the admin domain list.

#### Scenario: Request submits as an async command

- **WHEN** an owner submits a valid request
- **THEN** the console SHALL POST the registration (including `owningTeam`) and show the accepted command with its request ID
- **AND THEN** the new domain SHALL appear in the caller's list as pending

#### Scenario: Owner edits only the mutable fields

- **WHEN** an owner saves changes from the domain detail edit form
- **THEN** the console SHALL submit only description, emails, and cost center
- **AND THEN** name, account, and environments SHALL have no editable affordance

#### Scenario: Cost center edit is pre-filled and validated

- **WHEN** an owner opens the domain detail edit form
- **THEN** the cost center field SHALL be pre-filled with the current value
- **AND THEN** an empty cost center SHALL be rejected client-side as the API contract requires

#### Scenario: Platform admin disables a domain

- **WHEN** a platform admin chooses disable on an ACTIVE domain and confirms
- **THEN** the console SHALL call the state endpoint and reflect INACTIVE
- **AND THEN** non-admin users SHALL never see the affordance
