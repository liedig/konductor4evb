## MODIFIED Requirements

### Requirement: DynamoDB data model

The control plane SHALL use Amazon DynamoDB as the system of record with a single domains table for domain lifecycle management. The domain record SHALL store all metadata, approval-phase fields, and provisioned resource references. The DynamoDB SDK SHALL be used directly without an ORM or mapper layer. The partition key SHALL be the domain name (`domainName`). The table SHALL have `by-owner` and `by-status` Global Secondary Indexes. There SHALL NOT be a `by-name` GSI. The domain record SHALL store `secondaryOwnerEmails` as a String Set (SS) attribute.

#### Scenario: Single domains table design

- **WHEN** a domain is created via the register command
- **THEN** a single record SHALL be created in the domains table with `domainName` as the partition key, containing all domain metadata including `secondaryOwnerEmails` as a String Set
- **AND THEN** approval-phase fields (`taskToken`, `decidedBy`, `justification`, timestamps) SHALL be added to the same record as the workflow progresses
- **AND THEN** provisioned resource ARNs SHALL be added to the same record when provisioning completes

#### Scenario: GSI-based querying

- **WHEN** an operation requires listing domains by owner or status
- **THEN** the control plane SHALL query the appropriate GSI (`by-owner`, `by-status`) on the domains table
- **AND THEN** compose the response in the Lambda function
- **AND THEN** name-based lookups SHALL use a direct `GetItem` on the partition key, not a GSI query
