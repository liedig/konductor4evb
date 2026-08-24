## MODIFIED Requirements

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
