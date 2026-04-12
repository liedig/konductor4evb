# system-architecture Specification

## Purpose
TBD - created by archiving change architecture-decisions. Update Purpose after archive.
## Requirements
### Requirement: API layer

The control plane SHALL expose a REST API through Amazon API Gateway (REST API type) with IAM authentication. The API contract SHALL be defined using OpenAPI specification. Read operations (GET) SHALL be backed by dedicated Lambda functions invoked directly via `aws_proxy` integration. Write operations (POST, PUT) SHALL use `aws` service integrations to route commands through SQS queues for durable, asynchronous processing. API Gateway SHALL enforce structural request body validation on all write operations using JSON schema models defined in the OpenAPI spec.

#### Scenario: Read operations served synchronously via Lambda

- **WHEN** an authenticated client sends a GET request to any control plane read endpoint
- **THEN** API Gateway SHALL route the request directly to the dedicated Lambda function for that operation via `aws_proxy` integration
- **AND THEN** the Lambda function SHALL process the request and return a response conforming to the OpenAPI schema synchronously

#### Scenario: Write operations enqueued via SQS service integration

- **WHEN** an authenticated client sends a POST or PUT request to any control plane write endpoint
- **THEN** API Gateway SHALL validate the request body against the JSON schema model
- **AND THEN** on validation success, deliver the message to the appropriate SQS command queue via `aws` service integration
- **AND THEN** return `202 Accepted` with `{"message":"OK"}` without waiting for command processing

#### Scenario: API contract defined in OpenAPI

- **WHEN** a new API operation is added to the control plane
- **THEN** the operation SHALL be defined in the OpenAPI specification before implementation
- **AND THEN** API Gateway request validation SHALL enforce the schema for incoming requests

### Requirement: Lambda runtime and tooling

All Lambda functions SHALL be compiled with cargo-lambda and deployed using the `provided.al2023` custom runtime. Functions SHALL use the raw Lambda event handler (not an HTTP framework adapter). Each function SHALL be a separate binary target.

#### Scenario: Lambda function deployment

- **WHEN** a Lambda function is built and deployed
- **THEN** it SHALL be compiled as a standalone binary using cargo-lambda targeting `provided.al2023`
- **AND THEN** the function SHALL handle events directly via the Lambda Runtime API without an HTTP framework adapter

#### Scenario: Function isolation

- **WHEN** multiple API operations exist in the control plane
- **THEN** each operation SHALL be a separate Lambda function with its own binary
- **AND THEN** functions SHALL share common library crates but compile independently

### Requirement: DynamoDB data model

The control plane SHALL use Amazon DynamoDB as the system of record with a table-per-entity design. Each major entity type (domains, registration requests, subscriptions, schemas) SHALL have its own DynamoDB table. The DynamoDB SDK SHALL be used directly without an ORM or mapper layer.

#### Scenario: Table-per-entity design

- **WHEN** a new entity type is introduced to the control plane
- **THEN** it SHALL be stored in its own dedicated DynamoDB table
- **AND THEN** access to that table SHALL use the `aws-sdk-dynamodb` crate directly (raw API)

#### Scenario: Entity relationships across tables

- **WHEN** an operation requires data from multiple entity types (e.g., describe-domain needs domain metadata and resource ARNs)
- **THEN** the control plane SHALL perform separate queries to each relevant table
- **AND THEN** compose the response in the Lambda function

### Requirement: Error handling strategy

Library crates SHALL use `thiserror` for typed, structured error definitions. Lambda function handlers SHALL use `anyhow` for internal error handling where detailed error types are not essential. Error responses to API callers SHALL provide simplified, safe messages without exposing internal implementation details.

#### Scenario: Library error propagation

- **WHEN** a library crate encounters an error condition
- **THEN** it SHALL return a typed error defined with `thiserror`
- **AND THEN** the error type SHALL carry enough context for the caller to decide on the appropriate response

#### Scenario: API error responses

- **WHEN** a Lambda function encounters an internal error
- **THEN** it SHALL log the full error chain internally
- **AND THEN** return a simplified error response to the API caller that does not expose internal details, stack traces, or infrastructure information

### Requirement: Infrastructure as Code

All control plane infrastructure SHALL be defined using AWS SAM (Serverless Application Model). SAM templates SHALL define Lambda functions, API Gateway, DynamoDB tables, Step Functions state machines, IAM roles, and all supporting resources.

#### Scenario: SAM template deployment

- **WHEN** the control plane is deployed to an AWS environment
- **THEN** deployment SHALL use `sam build` and `sam deploy` (or equivalent CI/CD pipeline)
- **AND THEN** all resources SHALL be created and configured from the SAM template without manual AWS console operations

#### Scenario: Multi-environment support

- **WHEN** the control plane is deployed to different environments (dev, staging, prod)
- **THEN** the SAM template SHALL support parameterized configuration for environment-specific values (e.g., table names, bus names, namespace prefixes)

### Requirement: Control plane event bus

The control plane SHALL have a dedicated Amazon EventBridge event bus for internal lifecycle events. This bus SHALL be used to trigger control plane functionality (e.g., provisioning workflows) and SHALL NOT be exposed to application teams.

#### Scenario: Lifecycle event emission

- **WHEN** the control plane performs a state-changing operation (e.g., domain registration approved, domain creation succeeded)
- **THEN** it SHALL emit a lifecycle event to the control plane event bus with the event type and minimum payload defined in the domain-management spec

#### Scenario: Workflow triggering via events

- **WHEN** a lifecycle event is emitted to the control plane event bus (e.g., DomainRegistrationApproved)
- **THEN** an EventBridge rule on the control plane bus SHALL match the event and trigger the appropriate workflow (e.g., provisioning Step Functions)

