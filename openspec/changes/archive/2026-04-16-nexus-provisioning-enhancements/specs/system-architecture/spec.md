## ADDED Requirements

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

## MODIFIED Requirements

### Requirement: Control plane event bus

EventBridge Nexus SHALL have a dedicated Amazon EventBridge event bus for internal lifecycle events. This bus SHALL be named `{org}-{env}-nexus-bus`. This bus SHALL be used to trigger Nexus functionality (e.g., provisioning workflows) and SHALL NOT be exposed to application teams.

#### Scenario: Lifecycle event emission

- **WHEN** the Nexus performs a state-changing operation (e.g., domain registration approved, domain creation succeeded)
- **THEN** it SHALL emit a lifecycle event to the Nexus event bus with source prefix `nexus.` and the event type and minimum payload defined in the domain-management spec

#### Scenario: Workflow triggering via events

- **WHEN** a lifecycle event is emitted to the Nexus event bus (e.g., DomainRegistrationApproved)
- **THEN** an EventBridge rule on the Nexus bus SHALL match the event and trigger the appropriate workflow (e.g., provisioning Step Functions)

### Requirement: Infrastructure as Code

All Nexus infrastructure SHALL be defined using AWS SAM (Serverless Application Model). SAM templates SHALL define Lambda functions, API Gateway, DynamoDB tables, Step Functions state machines, IAM roles, SES configuration, SSM parameters, RAM resource shares, and all supporting resources. Step Functions state machine definitions SHALL be authored in YAML format using JSONata as the query language. All resource names SHALL follow the `{org}-{env}-{domain}` naming convention.

#### Scenario: SAM template deployment

- **WHEN** the Nexus is deployed to an AWS environment
- **THEN** deployment SHALL use `sam build` and `sam deploy` (or equivalent CI/CD pipeline)
- **AND THEN** all resources SHALL be created and configured from the SAM template without manual AWS console operations

#### Scenario: Step Functions authored in YAML with JSONata

- **WHEN** a Step Functions state machine is defined
- **THEN** the definition SHALL be in YAML format (`.asl.yaml` extension)
- **AND THEN** the definition SHALL use `QueryLanguage: JSONata` at the top level
- **AND THEN** data transformations SHALL use JSONata expressions (`{% ... %}`) instead of JSONPath

#### Scenario: Multi-environment support

- **WHEN** the Nexus is deployed to different environments (dev, staging, prod)
- **THEN** the SAM template SHALL support parameterised configuration for environment-specific values (e.g., table names, bus names, namespace prefixes, SES email addresses)

## RENAMED Requirements

### Requirement: SES email service dependency

- **FROM:** "The control plane SHALL use Amazon SES"
- **TO:** "EventBridge Nexus SHALL use Amazon SES"
