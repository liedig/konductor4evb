## ADDED Requirements

### Requirement: Domain count is capped

Domain registration SHALL enforce a ceiling of 1,000 domains per platform deployment. Creating a new domain beyond the ceiling SHALL fail with `409 Conflict` and a limit-exceeded reason, and SHALL NOT create the domain row or start the approval workflow; the failure SHALL be reported through the established `RegisterDomainFailed` outcome event carrying the limit reason. The ceiling check SHALL be atomic with domain creation so concurrent registrations cannot exceed it. Re-registration of a rejected domain and updates to existing domains SHALL NOT be affected by the ceiling.

#### Scenario: Registration beyond the cap is rejected

- **WHEN** 1,000 domains exist
- **AND WHEN** a caller registers a new domain
- **THEN** the API SHALL return `409 Conflict` with a limit-exceeded reason
- **AND THEN** no domain row SHALL be created, no approval workflow SHALL start, and a `RegisterDomainFailed` event SHALL carry the limit reason

#### Scenario: Concurrent registrations cannot race past the cap

- **WHEN** 999 domains exist
- **AND WHEN** two registrations for different new domains commit concurrently
- **THEN** exactly one SHALL succeed
- **AND THEN** the other SHALL return `409 Conflict` with a limit-exceeded reason

#### Scenario: Re-registering a rejected domain is not capped

- **WHEN** 1,000 domains exist and one of them is rejected
- **AND WHEN** a caller re-registers the rejected domain
- **THEN** the ceiling SHALL NOT block the re-registration
