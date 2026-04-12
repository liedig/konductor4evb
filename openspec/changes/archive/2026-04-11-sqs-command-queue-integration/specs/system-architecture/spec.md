## MODIFIED Requirements

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
