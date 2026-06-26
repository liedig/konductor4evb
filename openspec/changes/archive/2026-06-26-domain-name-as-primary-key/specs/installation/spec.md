## MODIFIED Requirements

### Requirement: Smoke test walkthrough

The installation guide SHALL include a smoke-test section that exercises the core domain lifecycle: register a domain, approve it, and verify ACTIVE status with provisioned resources. The smoke test SHALL use the domain name as the domain identifier throughout — the consumer provides the domain name at registration time and uses it directly for subsequent API calls without needing to discover a surrogate identifier.

#### Scenario: Smoke test covers domain lifecycle

- **WHEN** the operator follows the smoke-test section
- **THEN** the guide SHALL provide commands for registering a domain via the API using the domain name in the request body
- **AND THEN** the guide SHALL provide commands for approving the domain via the API using the task token
- **AND THEN** the guide SHALL describe how to verify the domain reached ACTIVE status by calling `GET /domains/{domainName}`
- **AND THEN** the guide SHALL describe how to verify that resources were provisioned in both accounts
- **AND THEN** the smoke test SHALL NOT require polling DynamoDB to discover a domain identifier
