## MODIFIED Requirements

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

### Requirement: API layer

The control plane SHALL expose a REST API through Amazon API Gateway (REST API type) with IAM authentication. The API contract SHALL be defined using OpenAPI specification. Read operations (GET) and synchronous action endpoints SHALL be backed by dedicated Lambda functions invoked directly via `aws_proxy` integration. Write operations (POST on `/domains`, PUT on `/domains/{domainId}`) SHALL use `aws` service integrations to route commands through SQS queues for durable, asynchronous processing. The decide-registration endpoint (`POST /domains/registrations/decide`) SHALL use `aws_proxy` Lambda integration for synchronous processing. API Gateway SHALL enforce structural request body validation on all write operations using JSON schema models defined in the OpenAPI spec. The API SHALL be served from a static stage named `v1`.

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

## REMOVED Requirements

### Requirement: Multi-environment support (for Nexus platform)

**Reason**: Nexus is an integration platform deployed once per organisation. It does not have a notion of `dev`/`staging`/`prod` at the platform layer; that concept belongs to the domains it provisions. The prior "Multi-environment support" scenario under the Infrastructure-as-Code requirement is superseded by the clearer split between platform-resource naming (fixed `{org}-nexus-*`) and domain-resource naming (configurable, env-scoped) documented in the modified Infrastructure as Code requirement above.

**Migration**: None at the spec level. At the code level, see the `nexus-platform-drop-environment` change tasks.
