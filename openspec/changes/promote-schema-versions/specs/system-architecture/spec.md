# Spec Delta

## ADDED Requirements

### Requirement: Environment list order defines the promotion pipeline

The order of the domain environment list SHALL define the schema promotion pipeline. The first entry SHALL be the authoring environment, where schemas are created and versions are appended. Each later entry SHALL receive versions only by promotion from the entry immediately before it. Promotion SHALL NOT skip environments. Because the list is fixed at setup, the pipeline order SHALL also be fixed for an installation.

#### Scenario: Default list forms a three-stage pipeline

- **WHEN** the environment list is `["dev", "staging", "prod"]`
- **THEN** `dev` SHALL be the authoring environment
- **AND THEN** `staging` SHALL be promoted from `dev`, and `prod` from `staging`

#### Scenario: Custom environment names follow list order

- **WHEN** the environment list is `["sandbox", "qa", "live"]`
- **THEN** `sandbox` SHALL be the authoring environment, `qa` SHALL be promoted from `sandbox`, and `live` from `qa`

#### Scenario: Single-environment installation

- **WHEN** the environment list has one entry
- **THEN** that entry SHALL be the authoring environment
- **AND THEN** no promotion target SHALL exist

#### Scenario: Pipeline order cannot change after setup

- **WHEN** an operator redeploys with the same environments in a different order
- **THEN** deployment tooling SHALL refuse the change as a changed fixed-at-setup setting

## MODIFIED Requirements

### Requirement: API layer

The control plane SHALL expose separate Amazon API Gateway REST API surfaces for AWS_IAM-authenticated callers and Cognito OAuth-authenticated interactive users. The OAuth surface SHALL use the Konductor REQUEST Lambda authorizer and SHALL NOT use an API Gateway native Cognito user-pool authorizer. The API contracts SHALL be defined using OpenAPI specifications. Routes SHALL be assigned to the surface appropriate to their caller and SHALL NOT combine OAuth and IAM as interchangeable authorization mechanisms.

Read operations and synchronous action endpoints SHALL be backed by dedicated Lambda functions invoked directly via `aws_proxy` integration. Asynchronous write operations SHALL preserve `aws` service integrations that route commands directly through SQS queues. API Gateway mapping templates SHALL derive trusted principal context from the authenticated request context and inject it into queued commands. API Gateway SHALL enforce structural request body validation on all write operations using JSON schema models defined in the OpenAPI contract. Each API SHALL serve one stage per entry of the configured environment list, named after that entry and sharing one deployment; the stages SHALL be derived from the environment list at deploy time, never from a fixed set of names. The stage name SHALL be the environment selector for publish, schema write, and schema promotion operations, and environment-agnostic operations SHALL behave identically on every stage. Domain-specific endpoints (`GET /domains/{domainName}`, `PUT /domains/{domainName}`) SHALL use the domain name as the path parameter.

The registration decision endpoint (`POST /domains/registrations/decide`) SHALL exist only on the OAuth surface, require `platform-admin`, and use `aws_proxy` Lambda integration for synchronous processing. AWS-native publishing, cross-account provisioning, and EventBridge delivery SHALL remain IAM based and outside the OAuth control-plane identity mechanism.

#### Scenario: OAuth read operation served synchronously via Lambda

- **WHEN** an OAuth-authenticated interactive client sends a GET request to an OAuth control-plane read endpoint
- **THEN** the REQUEST authorizer SHALL validate the Cognito access token and route access
- **AND THEN** API Gateway SHALL route the request to the dedicated Lambda function via `aws_proxy` integration with trusted principal context

#### Scenario: IAM read operation preserves AWS identity

- **WHEN** an AWS_IAM-authenticated client sends a GET request to an IAM control-plane read endpoint
- **THEN** API Gateway SHALL route the request to the dedicated Lambda function via `aws_proxy` integration
- **AND THEN** the trusted principal context SHALL use the caller's AWS account as `principalId` and resource-binding identity
- **AND THEN** an IAM principal ARN MAY be retained only as audit provenance

#### Scenario: Write operation enqueued through direct SQS integration

- **WHEN** an authenticated client sends a POST, PUT, PATCH, or DELETE request to an SQS-backed write endpoint
- **THEN** API Gateway SHALL validate the request body where applicable
- **AND THEN** on validation success, deliver the message directly to the appropriate SQS command queue via `aws` service integration
- **AND THEN** the mapping template SHALL inject compact trusted principal context derived from the API Gateway authentication context, including OAuth team-group identifiers or the IAM account identity as applicable
- **AND THEN** the trusted principal context SHALL NOT include resource entitlements
- **AND THEN** API Gateway SHALL return `202 Accepted` without waiting for command processing or resource authorization

#### Scenario: Approval action is OAuth-only

- **WHEN** a client sends a POST request to `/domains/registrations/decide`
- **THEN** the route SHALL require an OAuth principal with `platform-admin`
- **AND THEN** API Gateway SHALL validate the request body against the JSON schema model
- **AND THEN** route the request to the decide Lambda via `aws_proxy` integration with trusted principal context
- **AND THEN** no equivalent unauthenticated or IAM approval route SHALL authorize a decision

#### Scenario: API contracts defined in OpenAPI

- **WHEN** a new API operation is added to either control-plane surface
- **THEN** the operation SHALL be defined in that surface's OpenAPI specification before implementation
- **AND THEN** API Gateway request validation SHALL enforce the schema for incoming requests where a body is present

#### Scenario: APIs served from environment-named stages

- **WHEN** the control-plane APIs are deployed with the default environment list
- **THEN** each API SHALL serve stages named `dev`, `staging`, and `prod` from one deployment
- **AND THEN** publish, schema write, and schema promotion operations SHALL infer the target environment from the stage name
- **AND THEN** environment-agnostic operations SHALL behave identically on every stage

#### Scenario: Stages follow a custom environment list

- **WHEN** the control-plane APIs are deployed with the environment list `["sandbox", "qa", "live"]`
- **THEN** each API SHALL serve stages named `sandbox`, `qa`, and `live`
- **AND THEN** no stage named `dev`, `staging`, or `prod` SHALL exist
