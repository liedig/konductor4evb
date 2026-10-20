# system-architecture Specification

## Purpose

TBD - created by archiving change architecture-decisions. Update Purpose after archive.

## Requirements

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

All Konductor infrastructure SHALL be defined using AWS SAM (Serverless Application Model). SAM templates SHALL define Lambda functions, API Gateway, DynamoDB tables, Step Functions state machines, IAM roles, SES configuration, SSM parameters, RAM resource shares, and all supporting resources. Step Functions state machine definitions SHALL be authored in YAML format using JSONata as the query language.

Platform-account resources (the Konductor control plane) SHALL be named using the fixed `{org}-konductor-*` pattern. Konductor is a singleton per organisation and does NOT have an `Environment` concept at the platform layer. Domain resources provisioned by Konductor (event buses, schema registries, DLQs, dashboards, alarms) SHALL follow the configurable `{org}-{env}-{domain}-*` pattern, because a single domain has a distinct set of resources per environment.

#### Scenario: SAM template deployment

- **WHEN** Konductor is deployed to an AWS environment
- **THEN** deployment SHALL use `sam build` and `sam deploy` (or equivalent CI/CD pipeline)
- **AND THEN** all resources SHALL be created and configured from the SAM template without manual AWS console operations

#### Scenario: Step Functions authored in YAML with JSONata

- **WHEN** a Step Functions state machine is defined
- **THEN** the definition SHALL be in YAML format (`.asl.yaml` extension)
- **AND THEN** the definition SHALL use `QueryLanguage: JSONata` at the top level
- **AND THEN** data transformations SHALL use JSONata expressions (`{% ... %}`) instead of JSONPath

#### Scenario: Platform resources use fixed naming

- **WHEN** the Konductor platform stack is deployed
- **THEN** every platform resource (DynamoDB table, platform event bus, API Gateway, Lambda functions, IAM roles, queues, DLQs, log groups, dashboards, alarms) SHALL be named `{org}-konductor-<thing>`
- **AND THEN** no platform resource name SHALL include an environment segment

#### Scenario: Domain resources use configurable env-scoped naming

- **WHEN** Konductor provisions a domain in a given environment
- **THEN** all domain-owned resources SHALL be named according to the configurable pattern defaulting to `{org}-{env}-{domain}-<suffix>`
- **AND THEN** the same domain provisioned across multiple environments SHALL produce disjoint resource sets with distinct names

### Requirement: Control plane event bus

Konductor for Amazon EventBridge SHALL have a dedicated Amazon EventBridge event bus for internal lifecycle events. This bus SHALL be named `{org}-konductor-bus`. The bus SHALL be used to trigger Konductor functionality (e.g., provisioning workflows) and SHALL NOT be exposed to application teams.

#### Scenario: Lifecycle event emission

- **WHEN** Konductor performs a state-changing operation (e.g., domain registration approved, domain creation succeeded)
- **THEN** it SHALL emit a lifecycle event to the Konductor event bus with source prefix `konductor.` and the event type and minimum payload defined in the domain-management spec

#### Scenario: Workflow triggering via events

- **WHEN** a lifecycle event is emitted to the Konductor event bus (e.g., `DomainRegistrationApproved`)
- **THEN** an EventBridge rule on the Konductor bus SHALL match the event and trigger the appropriate workflow (e.g., provisioning Step Functions)

### Requirement: SES email service dependency

Konductor for Amazon EventBridge SHALL use Amazon SES for sending email notifications to domain owners and platform team members. SES sender and recipient addresses SHALL be parameterised in the SAM template. Konductor for Amazon EventBridge SHALL support SES sandbox mode for development environments.

#### Scenario: SES resources defined in SAM template

- **WHEN** the Konductor is deployed
- **THEN** the SAM template SHALL define parameters for `ApprovalFromEmail` and `ApprovalToEmail`
- **AND THEN** notification Lambda functions SHALL receive these addresses as environment variables
- **AND THEN** Lambda execution roles SHALL include `ses:SendEmail` permissions

### Requirement: SSM Parameter Store dependency

Konductor for Amazon EventBridge SHALL use AWS Systems Manager Parameter Store (Advanced tier) to store domain namespace identifiers. Parameters under `/namespace/*` SHALL be managed by the provisioning workflow. The SAM template SHALL define IAM permissions for SSM operations scoped to the `/namespace/*` path.

#### Scenario: SSM permissions defined in SAM template

- **WHEN** the Konductor is deployed
- **THEN** the provisioning workflow role SHALL include permissions for `ssm:PutParameter`, `ssm:GetParameter`, and `ssm:AddTagsToResource` scoped to `arn:aws:ssm:*:${AccountId}:parameter/namespace/*`

### Requirement: AWS RAM dependency

Konductor for Amazon EventBridge SHALL use AWS Resource Access Manager to share domain namespace SSM parameters with the AWS Organisation. RAM operations SHALL be performed by the provisioning workflow. The SAM template SHALL define IAM permissions for RAM share management.

#### Scenario: RAM permissions defined in SAM template

- **WHEN** the Konductor is deployed
- **THEN** the provisioning workflow role SHALL include permissions for `ram:CreateResourceShare`, `ram:GetResourceShares`, `ram:AssociateResourceShare`, and `ram:ListResources`

#### Scenario: AWS Organisations required for RAM

- **WHEN** the Konductor provisioning workflow attempts to create a RAM resource share with the Organisation as principal
- **THEN** the AWS account SHALL be a member of an AWS Organisation with RAM sharing enabled
- **AND THEN** if RAM sharing is not enabled, the workflow SHALL fail with a clear error message

### Requirement: Account-per-domain design principle

The system SHALL assume one AWS account per domain for application-side resources by default, and SHALL allow a registration to map individual environments to other accounts (account-per-domain-per-environment). The trust stack is deployed per distinct account used by any approved domain. Resource naming (`{org}-{env}-{domain}-*`) is unaffected by the mapping because names are environment-qualified, and no two domains share an account-environment pair. This principle SHALL be documented in the system architecture specification so that future changes do not silently violate it.

#### Scenario: Architecture documentation references account-per-domain

- **WHEN** a reader views the system architecture documentation
- **THEN** the account-per-domain-per-environment principle SHALL be documented as an explicit design decision
- **AND THEN** the documentation SHALL explain that each environment's application-side resources are provisioned in the account resolved for that environment, with the domain's default owner account used wherever no override exists

### Requirement: Platform configuration contract

The platform SHALL define a configuration contract covering every customer-supplied setting: organisation identifier, AWS Organization ARN, domain environment list, resource namespace pattern, notification sender and operator recipient addresses, access-token validity, and OAuth callback/logout URLs. Each setting SHALL be classified as fixed-at-setup or updateable. The organisation identifier, AWS Organization ARN, domain environment list, and namespace pattern SHALL be fixed at setup. Notification addresses, access-token validity, and OAuth URLs SHALL be updateable by redeployment with new values. The platform SHALL remain a single organisation-level control plane; the environment list SHALL describe domain environments only, not separate platform instances.

Deployment tooling SHALL refuse to apply a changed fixed-at-setup value to an existing installation and SHALL report which setting changed and that a new deployment is required. Tooling SHALL NOT rename, replace, orphan, or delete existing resources as a side effect of such a change.

#### Scenario: Fixed-at-setup change is rejected

- **WHEN** an operator deploys with an organisation identifier, environment list, namespace pattern, or AWS Organization ARN that differs from the deployed stack
- **THEN** deployment tooling SHALL refuse before any resource change
- **AND THEN** the refusal SHALL name the differing setting and state that changing it requires a new deployment

#### Scenario: Updateable setting changes by redeployment

- **WHEN** an operator redeploys with new notification addresses, token validity, or OAuth URLs
- **THEN** the new values SHALL be validated
- **AND THEN** the update SHALL apply without a fixed-at-setup rejection

#### Scenario: Single organisation-level control plane

- **WHEN** the environment list is configured
- **THEN** it SHALL describe the environments approved domains are provisioned into
- **AND THEN** it SHALL NOT create additional platform instances or environment-qualified platform resource names

### Requirement: Pre-deployment validation with actionable feedback

The platform SHALL provide a supported pre-deployment validation step that checks every contract setting and reports actionable errors before any deployment occurs. Validation SHALL cover: environment entry syntax, uniqueness, and count; namespace pattern required placeholders; organisation identifier syntax; AWS Organization ARN format; notification address pairing and SES verification prerequisites; and HTTPS for OAuth URLs outside local development. Deployment through the supported path SHALL NOT proceed when validation fails.

#### Scenario: Invalid environment entry fails before deploy

- **WHEN** the configuration contains an environment entry that violates the documented syntax
- **THEN** validation SHALL fail before any deployment
- **AND THEN** the error SHALL name the offending entry and the syntax rule

#### Scenario: Notifications are explicitly enabled or disabled

- **WHEN** exactly one notification address is configured, or a configured address is not verified in SES
- **THEN** validation SHALL fail with an actionable message
- **WHEN** both notification addresses are empty
- **THEN** validation SHALL pass and report that notifications are disabled

### Requirement: Customer configuration kept out of version control

The supported setup path SHALL keep customer-supplied values in a configuration file excluded from version control, with a committed example describing every setting. Validation and deployment SHALL consume the same file so they cannot read different values. For an existing installation without the file, the deployed stack's parameter values SHALL remain the record until the file is generated from them.

#### Scenario: Fresh setup from the committed example

- **WHEN** a customer copies the example configuration and fills in their values
- **THEN** validation and deployment SHALL read those values from that file
- **AND THEN** no customer value SHALL need to be committed to version control

#### Scenario: Existing installation adopts the configuration file

- **WHEN** an existing installation generates the configuration file from its deployed stack parameters
- **THEN** validation SHALL pass against the deployed stack
- **AND THEN** a subsequent redeployment with the generated file SHALL change no fixed-at-setup value

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
