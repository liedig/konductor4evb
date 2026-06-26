## ADDED Requirements

### Requirement: Account-per-domain design principle

The system SHALL assume one AWS account per domain for application-side resources. The trust stack is deployed per-account, and resource naming (`{org}-{env}-{domain}-*`) assumes no two domains share an account. This principle SHALL be documented in the system architecture specification so that future changes do not silently violate it.

#### Scenario: Architecture documentation references account-per-domain

- **WHEN** a reader views the system architecture documentation
- **THEN** the account-per-domain principle SHALL be documented as an explicit design decision
- **AND THEN** the documentation SHALL explain that each domain's application-side resources are provisioned in a single owner account

## MODIFIED Requirements

### Requirement: API layer

The control plane SHALL expose a REST API through Amazon API Gateway (REST API type) with IAM authentication. The API contract SHALL be defined using OpenAPI specification. Read operations (GET) and synchronous action endpoints SHALL be backed by dedicated Lambda functions invoked directly via `aws_proxy` integration. Write operations (POST on `/domains`, PUT on `/domains/{domainName}`) SHALL use `aws` service integrations to route commands through SQS queues for durable, asynchronous processing. The decide-registration endpoint (`POST /domains/registrations/decide`) SHALL use `aws_proxy` Lambda integration for synchronous processing. API Gateway SHALL enforce structural request body validation on all write operations using JSON schema models defined in the OpenAPI spec. The API SHALL be served from a static stage named `v1`. Domain-specific endpoints (`GET /domains/{domainName}`, `PUT /domains/{domainName}`) SHALL use the domain name as the path parameter.

#### Scenario: Read operations served synchronously via Lambda

- **WHEN** an authenticated client sends a GET request to any control plane read endpoint
- **THEN** API Gateway SHALL route the request directly to the dedicated Lambda function for that operation via `aws_proxy` integration
- **AND THEN** the Lambda function SHALL process the request and return a response conforming to the OpenAPI schema synchronously

#### Scenario: Write operations enqueued via SQS service integration

- **WHEN** an authenticated client sends a POST or PUT request to an SQS-backed write endpoint
- **THEN** API Gateway SHALL validate the request body against the JSON schema model
- **AND THEN** on validation success, deliver the message to the domains SQS command queue via `aws` service integration
- **AND THEN** return `202 Accepted` with `{"requestId":"<SQS-MessageId>","message":"..."}` without waiting for command processing

#### Scenario: Synchronous action endpoint served via Lambda

- **WHEN** an authenticated client sends a POST request to `/domains/registrations/decide`
- **THEN** API Gateway SHALL validate the request body against the JSON schema model
- **AND THEN** route the request to the decide Lambda function via `aws_proxy` integration
- **AND THEN** the Lambda SHALL process the request and return a synchronous response

#### Scenario: API contract defined in OpenAPI

- **WHEN** a new API operation is added to the control plane
- **THEN** the operation SHALL be defined in the OpenAPI specification before implementation
- **AND THEN** API Gateway request validation SHALL enforce the schema for incoming requests

#### Scenario: API served from the v1 stage

- **WHEN** the control plane is deployed
- **THEN** the API Gateway stage SHALL be named `v1`
- **AND THEN** the stage name SHALL NOT vary across deployments

### Requirement: DynamoDB data model

The control plane SHALL use Amazon DynamoDB as the system of record with a single domains table for domain lifecycle management. The domain record SHALL store all metadata, approval-phase fields, and provisioned resource references. The DynamoDB SDK SHALL be used directly without an ORM or mapper layer. The partition key SHALL be the domain name (`domainName`). The table SHALL have `by-owner` and `by-status` Global Secondary Indexes. There SHALL NOT be a `by-name` GSI.

#### Scenario: Single domains table design

- **WHEN** a domain is created via the register command
- **THEN** a single record SHALL be created in the domains table with `domainName` as the partition key, containing all domain metadata
- **AND THEN** approval-phase fields (`taskToken`, `decidedBy`, `justification`, timestamps) SHALL be added to the same record as the workflow progresses
- **AND THEN** provisioned resource ARNs SHALL be added to the same record when provisioning completes

#### Scenario: GSI-based querying

- **WHEN** an operation requires listing domains by owner or status
- **THEN** the control plane SHALL query the appropriate GSI (`by-owner`, `by-status`) on the domains table
- **AND THEN** compose the response in the Lambda function
- **AND THEN** name-based lookups SHALL use a direct `GetItem` on the partition key, not a GSI query
