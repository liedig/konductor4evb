# Spec Delta

## MODIFIED Requirements

### Requirement: Account-per-domain design principle

The system SHALL assume one AWS account per domain for application-side resources by default, and SHALL allow a registration to map individual environments to other accounts (account-per-domain-per-environment). The trust stack is deployed per distinct account used by any approved domain. Resource naming (`{org}-{env}-{domain}-*`) is unaffected by the mapping because names are environment-qualified, and no two domains share an account-environment pair. This principle SHALL be documented in the system architecture specification so that future changes do not silently violate it.

#### Scenario: Architecture documentation references account-per-domain

- **WHEN** a reader views the system architecture documentation
- **THEN** the account-per-domain-per-environment principle SHALL be documented as an explicit design decision
- **AND THEN** the documentation SHALL explain that each environment's application-side resources are provisioned in the account resolved for that environment, with the domain's default owner account used wherever no override exists
