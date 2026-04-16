# cross-account-trust-model Specification

## Purpose
TBD - created by archiving change architecture-decisions. Update Purpose after archive.
## Requirements
### Requirement: Control plane assumed role in application accounts

EventBridge Nexus SHALL use IAM assumed roles to create and manage resources in application accounts. Each application account that participates in the Nexus SHALL have a dedicated IAM role that the Nexus can assume. This role SHALL be created as part of the platform team's onboarding process for each application account.

#### Scenario: Nexus assumes role to create resources

- **WHEN** the provisioning workflow needs to create resources in an application account (domain event bus, integration rule DLQ)
- **THEN** the Nexus SHALL assume the trust role `{org}-{env}-nexus-trust-role` in the target application account using `sts:AssumeRole`
- **AND THEN** the assumed role session SHALL be used to create the required resources via the appropriate AWS service APIs

#### Scenario: Assumed role does not exist

- **WHEN** the Nexus attempts to assume a role in an application account and the role does not exist or the trust policy does not permit the assumption
- **THEN** the provisioning workflow step SHALL fail with a clear error indicating the missing or misconfigured trust role
- **AND THEN** the error SHALL include the expected role ARN (`{org}-{env}-nexus-trust-role`) and the required trust policy configuration
- **AND THEN** the saga compensation chain SHALL clean up any resources created before the failure

### Requirement: Trust role permissions for application accounts

The trust role in each application account SHALL grant the Nexus permissions to: create and delete EventBridge event buses, manage event bus policies, create and delete SQS queues (for DLQs), and create and manage IAM roles for bus-to-bus event delivery. All resource permissions SHALL be scoped to the `{org}-{env}-*` naming pattern.

#### Scenario: Trust role permits event bus management

- **WHEN** the Nexus assumes the trust role in an application account
- **THEN** the role SHALL permit `events:CreateEventBus`, `events:DeleteEventBus`, `events:DescribeEventBus`, `events:PutPermission`, and `events:RemovePermission` for domain event buses matching the `{org}-{env}-*` resource pattern

#### Scenario: Trust role permits DLQ management

- **WHEN** the Nexus assumes the trust role in an application account
- **THEN** the role SHALL permit `sqs:CreateQueue`, `sqs:DeleteQueue`, `sqs:SetQueueAttributes`, and `sqs:GetQueueAttributes` for DLQ queues matching the `{org}-{env}-*` resource pattern

#### Scenario: Trust role permits bus-to-bus role management

- **WHEN** the Nexus assumes the trust role in an application account
- **THEN** the role SHALL permit `iam:CreateRole`, `iam:DeleteRole`, `iam:PutRolePolicy`, and `iam:DeleteRolePolicy` for IAM roles used in bus-to-bus event delivery matching the `{org}-{env}-b2b-*` pattern

#### Scenario: Trust role follows least privilege

- **WHEN** the trust role is configured in an application account
- **THEN** the role's permissions SHALL be scoped to resources matching the `{org}-{env}-*` naming convention
- **AND THEN** the role SHALL NOT grant broad permissions such as `events:*` or `sqs:*` without resource constraints

### Requirement: Trust stack installation

Platform teams SHALL deploy a trust stack (CloudFormation/SAM template) in each application account before EventBridge Nexus can manage resources in that account. The Nexus project SHALL provide this template. The trust stack SHALL create the IAM trust role with the correct permissions and trust policy allowing the Nexus execution role in the platform account to assume it. The trust role SHALL be named using the `{org}-{env}-nexus-trust-role` convention.

#### Scenario: Trust stack deployment

- **WHEN** a platform team wants to onboard a new application account to the Nexus
- **THEN** they SHALL deploy the provided trust stack template in the application account
- **AND THEN** the template SHALL create an IAM role named `{org}-{env}-nexus-trust-role` with a trust policy that permits `sts:AssumeRole` from the Nexus execution role ARN in the platform account
- **AND THEN** the role SHALL have the permissions defined in the trust role permissions requirement

#### Scenario: Trust stack parameterisation

- **WHEN** the trust stack template is deployed
- **THEN** it SHALL accept parameters for: the Nexus execution role ARN (from the platform account), the provisioning workflow role ARN, the organisation identifier, the namespace convention prefix (for resource scoping), and the environment name
- **AND THEN** the trust role permissions SHALL be scoped using these parameters with the `{org}-{env}-*` resource pattern

### Requirement: Bus-to-bus assumed roles for subscription routing

When subscription rules route events from the platform domain bus to a subscriber's domain bus in another account, the delivery SHALL use a bus-to-bus assumed role. The Nexus SHALL create and manage these roles in the platform account. Each role SHALL permit EventBridge to call `events:PutEvents` on the target bus in the subscriber's application account.

#### Scenario: Bus-to-bus role created for subscription

- **WHEN** a subscription is created that routes events from a domain bus in the platform account to a subscriber's domain bus in an application account
- **THEN** the Nexus SHALL create (or reuse) an IAM role in the platform account that permits EventBridge to assume it
- **AND THEN** the role SHALL have a policy allowing `events:PutEvents` on the specific target event bus ARN in the subscriber's account

#### Scenario: Bus-to-bus role cleanup on unsubscribe

- **WHEN** a subscription is removed and no other subscriptions use the same bus-to-bus assumed role
- **THEN** the Nexus SHALL delete the assumed role and its associated policies
- **AND THEN** remaining subscriptions that share a role SHALL NOT be affected by unrelated unsubscribe operations

### Requirement: Control plane execution role

EventBridge Nexus Lambda functions and Step Functions workflows SHALL execute using IAM roles defined in the Nexus SAM template. These roles SHALL have permissions to: manage EventBridge buses and rules in the platform account, manage DynamoDB tables, manage SQS queues in the platform account, manage CloudWatch resources, manage schema registries, emit events to the Nexus event bus, start Step Functions executions, assume trust roles in application accounts, manage SSM parameters under `/namespace/*`, and manage RAM resource shares.

#### Scenario: Nexus role permits platform account operations

- **WHEN** a Lambda function or Step Functions workflow executes in the platform account
- **THEN** the execution role SHALL permit all operations required to manage domain resources in the platform account (EventBridge, SQS, CloudWatch, Schemas, DynamoDB, SSM, RAM)

#### Scenario: Nexus role permits cross-account assumption

- **WHEN** the provisioning workflow needs to create resources in an application account
- **THEN** the Nexus execution role SHALL permit `sts:AssumeRole` for trust roles in registered application accounts
- **AND THEN** the trust role ARN pattern SHALL be constrained to roles matching `{org}-{env}-nexus-trust-role`

#### Scenario: Nexus role permits SSM and RAM operations

- **WHEN** the provisioning workflow creates namespace parameters and manages RAM shares
- **THEN** the execution role SHALL permit `ssm:PutParameter`, `ssm:GetParameter`, and `ssm:AddTagsToResource` scoped to `arn:aws:ssm:*:${AccountId}:parameter/namespace/*`
- **AND THEN** the execution role SHALL permit `ram:CreateResourceShare`, `ram:GetResourceShares`, `ram:AssociateResourceShare`, and `ram:ListResources`

