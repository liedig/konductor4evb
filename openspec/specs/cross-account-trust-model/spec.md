# cross-account-trust-model Specification

## Purpose

TBD - created by archiving change architecture-decisions. Update Purpose after archive.

## Requirements

### Requirement: Control plane assumed role in domain accounts

Konductor for Amazon EventBridge SHALL use IAM assumed roles to create and manage resources in domain accounts. Each domain account that participates in Konductor SHALL have a single dedicated IAM role that Konductor can assume. The trust role SHALL be named `{org}-konductor-trust-role` (no environment segment) because one role serves all environments of all domains in that domain account. This is required by the one-domain-one-account model: a domain provisioned into environments `[dev, staging, prod]` places all three envs' resources in the same domain account, and Konductor uses one trust role to create them.

#### Scenario: Konductor assumes role to create resources across environments

- **WHEN** the provisioning workflow needs to create resources in a domain account for any environment of a domain
- **THEN** Konductor SHALL assume the trust role `{org}-konductor-trust-role` in the target domain account using `sts:AssumeRole`
- **AND THEN** the same assumed role SHALL be used regardless of which environment the resources belong to

#### Scenario: Assumed role does not exist

- **WHEN** Konductor attempts to assume a role in a domain account and the role does not exist or the trust policy does not permit the assumption
- **THEN** the provisioning workflow step SHALL fail with a clear error indicating the missing or misconfigured trust role
- **AND THEN** the error SHALL include the expected role ARN (`{org}-konductor-trust-role`) and the required trust policy configuration
- **AND THEN** the saga compensation chain SHALL clean up any resources created before the failure

### Requirement: Trust role permissions for domain accounts

The trust role in each domain account SHALL grant Konductor permissions to: create and delete EventBridge event buses, manage event bus policies, tag and untag the managed resources, create and delete SQS queues (for DLQs), and create and manage IAM roles for bus-to-bus event delivery. Resource permissions SHALL be scoped by the `{org}-*` prefix so that one role can manage per-env domain resources named `{org}-dev-*`, `{org}-staging-*`, `{org}-prod-*`, and any other env a domain may be provisioned into. The trust role itself has no environment segment.

#### Scenario: Trust role permits event bus management across envs

- **WHEN** Konductor assumes the trust role in a domain account
- **THEN** the role SHALL permit `events:CreateEventBus`, `events:DeleteEventBus`, `events:DescribeEventBus`, `events:PutPermission`, `events:RemovePermission`, and `events:TagResource`/`events:UntagResource` for domain event buses matching the `{org}-*` resource pattern, covering all envs

#### Scenario: Trust role permits DLQ management across envs

- **WHEN** Konductor assumes the trust role in a domain account
- **THEN** the role SHALL permit `sqs:CreateQueue`, `sqs:DeleteQueue`, `sqs:SetQueueAttributes`, `sqs:GetQueueAttributes`, and `sqs:TagQueue`/`sqs:UntagQueue` for DLQ queues matching the `{org}-*` resource pattern, covering all envs

#### Scenario: Trust role permits bus-to-bus role management across envs

- **WHEN** Konductor assumes the trust role in a domain account
- **THEN** the role SHALL permit `iam:CreateRole`, `iam:DeleteRole`, `iam:PutRolePolicy`, and `iam:DeleteRolePolicy` for IAM roles used in bus-to-bus event delivery matching the `{org}-*-b2b-*` pattern

#### Scenario: Trust role follows least privilege

- **WHEN** the trust role is configured in a domain account
- **THEN** the role's permissions SHALL be scoped to domain resources matching the `{org}-*` naming convention
- **AND THEN** the role SHALL NOT grant broad permissions such as `events:*` or `sqs:*` without resource constraints

### Requirement: Trust stack installation

Platform teams SHALL deploy a trust stack (CloudFormation/SAM template) in each domain account before Konductor for Amazon EventBridge can manage resources in that account. The Konductor project SHALL provide this template. The trust stack SHALL create the IAM trust role with the correct permissions and trust policy. The trust policy SHALL name the platform account root as principal and SHALL restrict assumption to exactly the Konductor execution role and the provisioning workflow role via an `ArnEquals` condition on `aws:PrincipalArn`, so that the constraint is string-matched per request rather than pinned to IAM unique principal IDs. The trust role SHALL be named `{org}-konductor-trust-role`.

#### Scenario: Trust stack deployment

- **WHEN** a platform team wants to onboard a new domain account to Konductor
- **THEN** they SHALL deploy the provided trust stack template in the domain account
- **AND THEN** the template SHALL create an IAM role named `{org}-konductor-trust-role` whose trust policy permits `sts:AssumeRole` from the platform account root, conditioned via `ArnEquals` on `aws:PrincipalArn` to exactly the Konductor execution role ARN and the provisioning workflow role ARN
- **AND THEN** the role SHALL have the permissions defined in the trust role permissions requirement

#### Scenario: Assumption restricted to the named platform roles

- **WHEN** any platform-account principal other than the Konductor execution role or the provisioning workflow role attempts `sts:AssumeRole` on the trust role
- **THEN** the assumption SHALL be denied by the `aws:PrincipalArn` condition

#### Scenario: Trust stack parameterisation

- **WHEN** the trust stack template is deployed
- **THEN** it SHALL accept parameters for: the Konductor execution role ARN (from the platform account), the provisioning workflow role ARN (from the platform account), and the organisation identifier
- **AND THEN** the trust stack SHALL NOT accept an environment parameter — the trust role name is environment-independent
- **AND THEN** the trust role permissions SHALL be scoped using the `{org}-*` resource pattern so the single role can manage domain resources across all environments

### Requirement: Platform delivery authorization on app-side buses

Each app-side domain event bus SHALL carry a resource policy permitting the **platform account** to call `events:PutEvents`, attached once at provisioning time via the trust role's existing `events:PutPermission` capability. This single grant SHALL authorize delivery for all current and future subscriptions targeting that bus — no per-subscription IAM roles or policies SHALL be created. Subscription rules in the platform account SHALL target app-side buses directly under this authorization.

#### Scenario: Policy attached during environment provisioning

- **WHEN** the per-environment provisioning workflow creates the app-side domain event bus in the domain account
- **THEN** it SHALL attach a resource policy to that bus permitting `events:PutEvents` from the platform account
- **AND THEN** the policy SHALL be attached using the trust role's existing `events:PutPermission` grant, requiring no trust-stack change

#### Scenario: Subscriptions require no additional authorization

- **WHEN** a subscription is created or removed for a consumer domain
- **THEN** no IAM roles, role policies, or resource policy changes SHALL be made in either account
- **AND THEN** unsubscribing SHALL leave the provision-time delivery policy in place for other and future subscriptions

### Requirement: Control plane execution role

Konductor for Amazon EventBridge Lambda functions and Step Functions workflows SHALL execute using IAM roles defined in the Konductor SAM template. These roles SHALL have permissions to: manage EventBridge buses and rules in the platform account, manage DynamoDB tables, manage SQS queues in the platform account, manage CloudWatch resources, manage schema registries, emit events to the Konductor event bus, start Step Functions executions, assume trust roles in domain accounts, manage SSM parameters under `/namespace/*`, and manage RAM resource shares.

#### Scenario: Konductor role permits platform account operations

- **WHEN** a Lambda function or Step Functions workflow executes in the platform account
- **THEN** the execution role SHALL permit all operations required to manage domain resources in the platform account (EventBridge, SQS, CloudWatch, Schemas, DynamoDB, SSM, RAM)

#### Scenario: Konductor role permits cross-account assumption

- **WHEN** the provisioning workflow needs to create resources in a domain account
- **THEN** the Konductor execution role SHALL permit `sts:AssumeRole` for trust roles in registered domain accounts
- **AND THEN** the trust role ARN pattern SHALL be constrained to roles matching `{org}-konductor-trust-role`

#### Scenario: Konductor role permits SSM and RAM operations

- **WHEN** the provisioning workflow creates namespace parameters and manages RAM shares
- **THEN** the execution role SHALL permit `ssm:PutParameter`, `ssm:GetParameter`, and `ssm:AddTagsToResource` scoped to `arn:aws:ssm:*:${AccountId}:parameter/namespace/*`
- **AND THEN** the execution role SHALL permit `ram:CreateResourceShare`, `ram:GetResourceShares`, `ram:AssociateResourceShare`, and `ram:ListResources`

### Requirement: Trust survives platform role recreation

The trust relationship between a domain account and the platform SHALL remain valid when the platform roles are deleted and recreated with the same names (for example during a core stack teardown and redeploy). Because both platform roles have deterministic names, their ARN strings are stable, and the `aws:PrincipalArn` condition SHALL continue to match without any change to the deployed trust stack.

#### Scenario: Core stack redeploy does not sever trust

- **WHEN** the platform core stack is torn down and redeployed, recreating the Konductor execution role and provisioning workflow role with new IAM unique principal IDs
- **THEN** the provisioning workflow SHALL still successfully assume `{org}-konductor-trust-role` in every domain account
- **AND THEN** no trust stack update or redeploy SHALL be required in any domain account

### Requirement: Organization-enforced managed infrastructure protection

Konductor SHALL provide a separately deployable organization-admin guardrail package using AWS Organizations SCPs to protect its application-account event buses and integration DLQs. When attached to participating member accounts, the guardrails SHALL deny unauthorized resource deletion, configuration and policy changes, queue purge, and protected ownership-identifier mutation, including operations by member-account administrators and root within SCP coverage. The guardrails SHALL preserve permitted integration rules and queue consumption and SHALL NOT grant IAM permissions themselves.

#### Scenario: Broad administrator permissions do not bypass protection

- **WHEN** a member-account principal with broad administrator permissions attempts a protected operation without matching an authorized platform exception
- **THEN** the attached SCP guardrail SHALL deny that operation despite the identity-based allow

#### Scenario: Resource identity cannot be removed to evade enforcement

- **WHEN** a local principal attempts to remove or forge ownership tags or recreate a protected resource in a reserved namespace
- **THEN** the guardrails SHALL prevent that action from bypassing the managed-resource restrictions

#### Scenario: Member accounts cannot shed protection through organization departure

- **WHEN** a local member-account principal attempts to leave the organization to escape the attached guardrails
- **THEN** the centrally administered guardrails SHALL deny that departure within the supported Organizations enforcement model

#### Scenario: Konductor lifecycle operations remain authorized

- **WHEN** Konductor uses the designated trusted platform path to provision, compensate, deprovision, or retag managed infrastructure
- **THEN** the guardrails SHALL permit the required operations subject to the existing role permissions
- **AND** unrelated local principals SHALL NOT inherit this exception

### Requirement: Protected platform exception identities

Platform exceptions SHALL identify exact configured roles rather than broad name patterns. Domain-account principals SHALL NOT be able to acquire an exception by modifying or recreating an exempt role, changing its trust or permissions policies, attaching policies, changing relevant managed-policy versions, passing the role to another service, or modifying associated ownership or boundary controls. The ordinary Konductor resource-management path SHALL NOT authorize self-modification of its trust policy.

#### Scenario: Local administrator cannot rewrite the trust role

- **WHEN** a local administrator attempts to change the exempt role's trust policy or effective permissions
- **THEN** the operation SHALL be denied outside the designated centrally controlled maintenance path

#### Scenario: Role replacement and passing do not create exceptions

- **WHEN** a local principal attempts to delete and recreate an exempt role, pass it to a service, or assume it without meeting the configured platform trust conditions
- **THEN** the attempt SHALL be denied

#### Scenario: Authorized role sessions retain the intended exception

- **WHEN** an authorized platform principal assumes the exact configured Konductor trust role
- **THEN** its session SHALL receive only the intended resource-management exception
- **AND** similarly named roles SHALL NOT receive that exception

### Requirement: Central recovery and explicit enforcement coverage

Guardrail installation SHALL require explicit organization-admin targeting and a verified centrally controlled recovery path that domain-account administrators cannot modify or assume. Normal domain registration and provisioning SHALL NOT attach or modify organization policies. Installation guidance SHALL distinguish SCP restrictions from application IAM grants, SHALL document management-account and service-linked-role exclusions, and SHALL NOT claim deployed enforcement based solely on local tests, console rendering, or trust-role presence.

#### Scenario: Recovery remains possible without local bypass

- **WHEN** a guardrail blocks a legitimate platform maintenance operation
- **THEN** centrally authorized operators SHALL have a documented procedure to repair policy or role configuration
- **AND** the procedure SHALL NOT grant domain administrators a standing bypass

#### Scenario: Enforcement evidence is missing

- **WHEN** only local policy tests have run or target-account attachment has not been verified
- **THEN** the installation SHALL be described as not yet verified for deployed enforcement
- **AND** separately authorized sandbox checks SHALL be required before claiming the protection operational

#### Scenario: An account or principal is outside SCP coverage

- **WHEN** a deployment uses the management account or an AWS-documented SCP-exempt principal path
- **THEN** the documented protection claim SHALL explicitly exclude that path
- **AND** the presence of this guardrail package SHALL NOT be presented as removing that AWS limitation
