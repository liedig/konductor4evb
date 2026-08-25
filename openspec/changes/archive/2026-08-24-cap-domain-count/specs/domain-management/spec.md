## ADDED Requirements

### Requirement: Domain count is capped

Domain registration SHALL enforce a ceiling of 1,000 domains per platform deployment. Creating a new domain beyond the ceiling SHALL fail deterministically: no domain row is created, no approval workflow starts, and the failure SHALL be reported through the established `RegisterDomainFailed` outcome event carrying a limit-exceeded reason, with the command message acknowledged (no retry, since retrying cannot change the outcome). The ceiling check SHALL be atomic with domain creation so concurrent registrations cannot exceed it. The ceiling SHALL apply to every new domain row, including system-rejected rows recorded by the registration collision preflight. Re-registration of a rejected domain and updates to existing domains SHALL NOT be affected by the ceiling.

#### Scenario: Registration beyond the cap is rejected

- **WHEN** 1,000 domains exist
- **AND WHEN** a caller registers a new domain
- **THEN** the registration SHALL fail with a limit-exceeded reason
- **AND THEN** no domain row SHALL be created, no approval workflow SHALL start, a `RegisterDomainFailed` event SHALL carry the limit reason, and the command message SHALL be acknowledged without retry

#### Scenario: Concurrent registrations cannot race past the cap

- **WHEN** 999 domains exist
- **AND WHEN** two registrations for different new domains commit concurrently
- **THEN** exactly one SHALL succeed
- **AND THEN** the other SHALL fail with the limit-exceeded reason through the same deterministic path

#### Scenario: Re-registering a rejected domain is not capped

- **WHEN** 1,000 domains exist and one of them is rejected
- **AND WHEN** a caller re-registers the rejected domain
- **THEN** the ceiling SHALL NOT block the re-registration
