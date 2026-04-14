## MODIFIED Requirements

### Requirement: API layer

The control plane SHALL expose a REST API through Amazon API Gateway (REST API type) with IAM authentication. The API contract SHALL be defined using OpenAPI specification. Read operations (GET) and synchronous action endpoints SHALL be backed by dedicated Lambda functions invoked directly via `aws_proxy` integration. Write operations (POST on `/domains`, PUT on `/domains/{domainId}`) SHALL use `aws` service integrations to route commands through SQS queues for durable, asynchronous processing. The decide-registration endpoint (`POST /domains/registrations/decide`) SHALL use `aws_proxy` Lambda integration for synchronous processing. API Gateway SHALL enforce structural request body validation on all write operations using JSON schema models defined in the OpenAPI spec.

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

### Requirement: DynamoDB data model

The control plane SHALL use Amazon DynamoDB as the system of record with a single domains table for domain lifecycle management. The domain record SHALL store all metadata, approval-phase fields, and provisioned resource references. The DynamoDB SDK SHALL be used directly without an ORM or mapper layer.

#### Scenario: Single domains table design

- **WHEN** a domain is created via the register command
- **THEN** a single record SHALL be created in the domains table containing all domain metadata
- **AND THEN** approval-phase fields (`taskToken`, `decidedBy`, `justification`, timestamps) SHALL be added to the same record as the workflow progresses
- **AND THEN** provisioned resource ARNs SHALL be added to the same record when provisioning completes

#### Scenario: GSI-based querying

- **WHEN** an operation requires listing domains by name, owner, or status
- **THEN** the control plane SHALL query the appropriate GSI (`by-name`, `by-owner`, `by-status`) on the domains table
- **AND THEN** compose the response in the Lambda function

### Requirement: Infrastructure as Code

All control plane infrastructure SHALL be defined using AWS SAM (Serverless Application Model). SAM templates SHALL define Lambda functions, API Gateway, DynamoDB tables, Step Functions state machines, IAM roles, SES configuration, and all supporting resources. Step Functions state machine definitions SHALL be authored in YAML format using JSONata as the query language.

#### Scenario: SAM template deployment

- **WHEN** the control plane is deployed to an AWS environment
- **THEN** deployment SHALL use `sam build` and `sam deploy` (or equivalent CI/CD pipeline)
- **AND THEN** all resources SHALL be created and configured from the SAM template without manual AWS console operations

#### Scenario: Step Functions authored in YAML with JSONata

- **WHEN** a Step Functions state machine is defined
- **THEN** the definition SHALL be in YAML format (`.asl.yaml` extension)
- **AND THEN** the definition SHALL use `QueryLanguage: JSONata` at the top level
- **AND THEN** data transformations SHALL use JSONata expressions (`{% ... %}`) instead of JSONPath

#### Scenario: Multi-environment support

- **WHEN** the control plane is deployed to different environments (dev, staging, prod)
- **THEN** the SAM template SHALL support parameterized configuration for environment-specific values (e.g., table names, bus names, namespace prefixes, SES email addresses)

## ADDED Requirements

### Requirement: SES email service dependency

The control plane SHALL use Amazon SES for sending email notifications to domain owners and platform team members. SES sender and recipient addresses SHALL be parameterised in the SAM template. The control plane SHALL support SES sandbox mode for development environments.

#### Scenario: SES resources defined in SAM template

- **WHEN** the control plane is deployed
- **THEN** the SAM template SHALL define parameters for `ApprovalFromEmail` and `ApprovalToEmail`
- **AND THEN** notification Lambda functions SHALL receive these addresses as environment variables
- **AND THEN** Lambda execution roles SHALL include `ses:SendEmail` permissions
