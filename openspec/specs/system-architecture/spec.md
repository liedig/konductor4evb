# system-architecture Specification

## Purpose

TBD - created by archiving change architecture-decisions. Update Purpose after archive.

## Requirements

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

All Nexus infrastructure SHALL be defined using AWS SAM (Serverless Application Model). SAM templates SHALL define Lambda functions, API Gateway, DynamoDB tables, Step Functions state machines, IAM roles, SES configuration, SSM parameters, RAM resource shares, and all supporting resources. Step Functions state machine definitions SHALL be authored in YAML format using JSONata as the query language.

Platform-account resources (the Nexus control plane) SHALL be named using the fixed `{org}-nexus-*` pattern. Nexus is a singleton per organisation and does NOT have an `Environment` concept at the platform layer. Domain resources provisioned by Nexus (event buses, schema registries, DLQs, dashboards, alarms) SHALL follow the configurable `{org}-{env}-{domain}-*` pattern, because a single domain has a distinct set of resources per environment.

#### Scenario: SAM template deployment

- **WHEN** Nexus is deployed to an AWS environment
- **THEN** deployment SHALL use `sam build` and `sam deploy` (or equivalent CI/CD pipeline)
- **AND THEN** all resources SHALL be created and configured from the SAM template without manual AWS console operations

#### Scenario: Step Functions authored in YAML with JSONata

- **WHEN** a Step Functions state machine is defined
- **THEN** the definition SHALL be in YAML format (`.asl.yaml` extension)
- **AND THEN** the definition SHALL use `QueryLanguage: JSONata` at the top level
- **AND THEN** data transformations SHALL use JSONata expressions (`{% ... %}`) instead of JSONPath

#### Scenario: Platform resources use fixed naming

- **WHEN** the Nexus platform stack is deployed
- **THEN** every platform resource (DynamoDB table, platform event bus, API Gateway, Lambda functions, IAM roles, queues, DLQs, log groups, dashboards, alarms) SHALL be named `{org}-nexus-<thing>`
- **AND THEN** no platform resource name SHALL include an environment segment

#### Scenario: Domain resources use configurable env-scoped naming

- **WHEN** Nexus provisions a domain in a given environment
- **THEN** all domain-owned resources SHALL be named according to the configurable pattern defaulting to `{org}-{env}-{domain}-<suffix>`
- **AND THEN** the same domain provisioned across multiple environments SHALL produce disjoint resource sets with distinct names

### Requirement: Control plane event bus

EventBridge Nexus SHALL have a dedicated Amazon EventBridge event bus for internal lifecycle events. This bus SHALL be named `{org}-nexus-bus`. The bus SHALL be used to trigger Nexus functionality (e.g., provisioning workflows) and SHALL NOT be exposed to application teams.

#### Scenario: Lifecycle event emission

- **WHEN** Nexus performs a state-changing operation (e.g., domain registration approved, domain creation succeeded)
- **THEN** it SHALL emit a lifecycle event to the Nexus event bus with source prefix `nexus.` and the event type and minimum payload defined in the domain-management spec

#### Scenario: Workflow triggering via events

- **WHEN** a lifecycle event is emitted to the Nexus event bus (e.g., `DomainRegistrationApproved`)
- **THEN** an EventBridge rule on the Nexus bus SHALL match the event and trigger the appropriate workflow (e.g., provisioning Step Functions)

### Requirement: SES email service dependency

EventBridge Nexus SHALL use Amazon SES for sending email notifications to domain owners and platform team members. SES sender and recipient addresses SHALL be parameterised in the SAM template. EventBridge Nexus SHALL support SES sandbox mode for development environments.

#### Scenario: SES resources defined in SAM template

- **WHEN** the Nexus is deployed
- **THEN** the SAM template SHALL define parameters for `ApprovalFromEmail` and `ApprovalToEmail`
- **AND THEN** notification Lambda functions SHALL receive these addresses as environment variables
- **AND THEN** Lambda execution roles SHALL include `ses:SendEmail` permissions

### Requirement: SSM Parameter Store dependency

EventBridge Nexus SHALL use AWS Systems Manager Parameter Store (Advanced tier) to store domain namespace identifiers. Parameters under `/namespace/*` SHALL be managed by the provisioning workflow. The SAM template SHALL define IAM permissions for SSM operations scoped to the `/namespace/*` path.

#### Scenario: SSM permissions defined in SAM template

- **WHEN** the Nexus is deployed
- **THEN** the provisioning workflow role SHALL include permissions for `ssm:PutParameter`, `ssm:GetParameter`, and `ssm:AddTagsToResource` scoped to `arn:aws:ssm:*:${AccountId}:parameter/namespace/*`

### Requirement: AWS RAM dependency

EventBridge Nexus SHALL use AWS Resource Access Manager to share domain namespace SSM parameters with the AWS Organisation. RAM operations SHALL be performed by the provisioning workflow. The SAM template SHALL define IAM permissions for RAM share management.

#### Scenario: RAM permissions defined in SAM template

- **WHEN** the Nexus is deployed
- **THEN** the provisioning workflow role SHALL include permissions for `ram:CreateResourceShare`, `ram:GetResourceShares`, `ram:AssociateResourceShare`, and `ram:ListResources`

#### Scenario: AWS Organisations required for RAM

- **WHEN** the Nexus provisioning workflow attempts to create a RAM resource share with the Organisation as principal
- **THEN** the AWS account SHALL be a member of an AWS Organisation with RAM sharing enabled
- **AND THEN** if RAM sharing is not enabled, the workflow SHALL fail with a clear error message

### Requirement: Account-per-domain design principle

The system SHALL assume one AWS account per domain for application-side resources. The trust stack is deployed per-account, and resource naming (`{org}-{env}-{domain}-*`) assumes no two domains share an account. This principle SHALL be documented in the system architecture specification so that future changes do not silently violate it.

#### Scenario: Architecture documentation references account-per-domain

- **WHEN** a reader views the system architecture documentation
- **THEN** the account-per-domain principle SHALL be documented as an explicit design decision
- **AND THEN** the documentation SHALL explain that each domain's application-side resources are provisioned in a single owner account
