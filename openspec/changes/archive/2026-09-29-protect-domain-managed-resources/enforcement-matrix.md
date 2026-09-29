# Enforcement Matrix: Action / Resource / Condition

This document is the authoritative action/resource/condition matrix for the
AWS Organizations SCP guardrails introduced by the
`protect-domain-managed-resources` change. Every row was checked against the
AWS Service Authorization Reference. Where AWS documentation shows a gap, the
gap is recorded rather than assumed away.

**Durable protection anchor.** All protected-resource selectors use ARN or
resource-name patterns, not tags alone. Tags can be removed; name patterns
cannot. The protected resource namespace is `{org}-*` (e.g., `acme-*` for
organization `acme`). The `{org}` prefix is an installation-time input bound
to the Konductor `Organization` parameter; the guardrail package substitutes
it into every SCP statement.

**Exception model.** A single `Deny` statement per protected surface carries
the resource selectors. A narrowly scoped exception on
`aws:PrincipalArn` allows the exact Konductor trust-role ARN
(`arn:aws:iam::<member-account>:role/{org}-konductor-trust-role`) and an
explicitly configured central recovery-role ARN to perform the protected
operation. No wildcard principal patterns (`*konductor*`) are used.

**Legend.**

- *Protection* — the classification this change assigns:
  - `PROTECTED` — SCP `Deny` with the ARN/name selector.
  - `ALLOWED` — intentionally left to team IAM (no SCP deny).
  - `CONDITIONAL` — denied only in specific configurations or with
    supplementary conditions; see *Notes*.
- *SCP-eligible* — whether SCPs support the action with the indicated
  resource types and condition keys, per the AWS Service Authorization
  Reference for the service.

---

## 1. EventBridge Event Bus Operations

Service prefix: `events`. Service Authorization Reference:
<https://docs.aws.amazon.com/service-authorization/latest/reference/list_amazoneventbridge.html>

### 1.1 Lifecycle

| Action | Resource types | Condition keys (relevant) | Batch equivalence | SCP-eligible | Protection | Notes |
|---|---|---|---|---|---|---|
| `events:CreateEventBus` | `event-bus` | `aws:RequestTag`, `aws:TagKeys`, `aws:ResourceTag`, `events:EventBusName` (via resource ARN) | — | Yes | PROTECTED | Deny on `arn:aws:events:*:<account>:event-bus/{org}-*`. Without this deny, a local admin could create `acme-evil-bus` and later claim it is Konductor-managed or collide with reserved names. |
| `events:DeleteEventBus` | `event-bus` | `aws:ResourceTag`, `events:EventBusName` (via resource ARN) | — | Yes | PROTECTED | Core lifecycle deny. |

### 1.2 Configuration

| Action | Resource types | Condition keys (relevant) | Batch equivalence | SCP-eligible | Protection | Notes |
|---|---|---|---|---|---|---|
| `events:DescribeEventBus` | `event-bus` | `aws:ResourceTag` | — | Yes | ALLOWED | Read-only. Teams need to inspect their bus. |
| `events:ListEventBuses` | (account) | — | — | Yes (no resource) | ALLOWED | List calls do not accept resource ARNs; SCPs can only filter by `aws:RequestedRegion` etc. Teams already need this for discovery. |
| `events:UpdateEventBus` *(if introduced)* | `event-bus` | `aws:ResourceTag` | — | Conditional on AWS release | PROTECTED | EventBridge did not expose `UpdateEventBus` at the time this matrix was authored. If AWS ships it, the guardrail package adds it to the same deny statement as `DeleteEventBus`. Tracked as a forward-coverage note in the template. |

### 1.3 Policies

| Action | Resource types | Condition keys (relevant) | Batch equivalence | SCP-eligible | Protection | Notes |
|---|---|---|---|---|---|---|
| `events:PutPermission` | `event-bus` | `aws:ResourceTag`, `events:EventBusName` (via resource ARN) | — | Yes | PROTECTED | Replaces the resource policy on the bus. Protected to prevent local principals from opening the bus to foreign accounts. |
| `events:RemovePermission` | `event-bus` | `aws:ResourceTag`, `events:EventBusName` (via resource ARN) | — | Yes | PROTECTED | Removes statements from the bus resource policy. Protected to prevent stripping the Konductor-managed access path. |

### 1.4 Tagging

| Action | Resource types | Condition keys (relevant) | Batch equivalence | SCP-eligible | Protection | Notes |
|---|---|---|---|---|---|---|
| `events:TagResource` | `event-bus` | — | — | Yes | PROTECTED | Unconditional deny on `{org}-*` event buses. Prevents adding, modifying, or forging ownership tags on Konductor-managed buses. Team-owned rules on the same bus are not affected — the resource selector matches only the bus ARN, not rule ARNs. |
| `events:UntagResource` | `event-bus` | — | — | Yes | PROTECTED | Same shape as `TagResource`. Prevents stripping ownership tags to evade name-based enforcement. |

### 1.5 Rules / Targets (team-owned, NOT protected)

These actions are the documented team surface. They are intentionally
ALLOWED by the SCP so application teams keep integration-rule ownership on
their domain bus. Application IAM policies still gate who can call them.

| Action | Resource types | Condition keys (relevant) | Batch equivalence | SCP-eligible | Protection | Notes |
|---|---|---|---|---|---|---|
| `events:PutRule` | `rule`, `event-bus` | `aws:ResourceTag`, `events:EventBusName` | — | Yes | ALLOWED | Team creates/updates own rules. |
| `events:DeleteRule` | `rule` | `aws:ResourceTag` | — | Yes | ALLOWED | Team deletes own rules. |
| `events:DisableRule` | `rule` | `aws:ResourceTag` | — | Yes | ALLOWED | Team toggles own rules. |
| `events:EnableRule` | `rule` | `aws:ResourceTag` | — | Yes | ALLOWED | Team toggles own rules. |
| `events:PutTargets` | `rule` | `aws:ResourceTag`, `events:TargetArn` | — | Yes | ALLOWED | Team attaches own targets. |
| `events:RemoveTargets` | `rule` | `aws:ResourceTag` | — | Yes | ALLOWED | Team detaches own targets. |
| `events:ListRules` | (account), `event-bus` | — | — | Yes | ALLOWED | Discovery. |
| `events:ListTargetsByRule` | `rule` | — | — | Yes | ALLOWED | Discovery. |
| `events:DescribeRule` | `rule` | `aws:ResourceTag` | — | Yes | ALLOWED | Read. |

> **Out-of-scope rule protection.** The platform subscription rules created
> by Konductor on the application bus live under a distinct name prefix
> (`{org}-konductor-*`). Those names fall under the reserved namespace, so
> the rule-management actions on them are covered by the same reserved-name
> deny used for `CreateEventBus`/`CreateQueue`. Team rules with other names
> are unaffected.

---

## 2. SQS Queue Operations

Service prefix: `sqs`. Service Authorization Reference:
<https://docs.aws.amazon.com/service-authorization/latest/reference/list_amazonsqs.html>

### 2.1 Lifecycle

| Action | Resource types | Condition keys (relevant) | Batch equivalence | SCP-eligible | Protection | Notes |
|---|---|---|---|---|---|---|
| `sqs:CreateQueue` | `queue` (ARN pattern from `QueueName`) | `aws:RequestTag`, `aws:TagKeys` | — | Yes | PROTECTED | Deny on `arn:aws:sqs:*:<account>:{org}-*`. Prevents reserved-name squatting. |
| `sqs:DeleteQueue` | `queue` | `aws:ResourceTag` | — | Yes | PROTECTED | Core lifecycle deny. |

### 2.2 Configuration

| Action | Resource types | Condition keys (relevant) | Batch equivalence | SCP-eligible | Protection | Notes |
|---|---|---|---|---|---|---|
| `sqs:GetQueueAttributes` | `queue` | `aws:ResourceTag` | — | Yes | ALLOWED | Read-only; teams need this to inspect DLQ depth and policy. |
| `sqs:SetQueueAttributes` | `queue` | `aws:ResourceTag` | — | Yes | PROTECTED | Single call surface for policy, redrive policy, KMS key, visibility, retention. Deny wholesale on `{org}-*` — see 2.4 for redrive specifics. |
| `sqs:GetQueueUrl` | `queue` | `aws:ResourceTag` | — | Yes | ALLOWED | Read-only lookup. |
| `sqs:ListQueues` | (account) | — | — | Yes (no resource) | ALLOWED | Prefix-scoped listing; no resource ARN available to SCP. |

### 2.3 Purge

| Action | Resource types | Condition keys (relevant) | Batch equivalence | SCP-eligible | Protection | Notes |
|---|---|---|---|---|---|---|
| `sqs:PurgeQueue` | `queue` | `aws:ResourceTag` | — | Yes | PROTECTED | Prohibited by default on Konductor-managed queues. Purge is destructive, non-recoverable, and indistinguishable from data loss in DLQ audit. Only the trust-role / recovery exception can purge. |

### 2.4 Redrive

| Action | Resource types | Condition keys (relevant) | Batch equivalence | SCP-eligible | Protection | Notes |
|---|---|---|---|---|---|---|
| Redrive configuration | `queue` | via `sqs:SetQueueAttributes` (`RedrivePolicy`, `RedriveAllowPolicy` attributes) | — | Yes | PROTECTED (via SetQueueAttributes) | SQS does not expose a discrete `SetRedrivePolicy` action. The deny on `sqs:SetQueueAttributes` covers redrive changes. There is no narrower SCP hook that would allow non-redrive attribute edits while blocking redrive edits; SCP conditions cannot introspect the request body's attribute names. The conservative deny is therefore required. |

### 2.5 Messaging (team-owned, NOT protected)

| Action | Resource types | Condition keys (relevant) | Batch equivalence | SCP-eligible | Protection | Notes |
|---|---|---|---|---|---|---|
| `sqs:SendMessage` | `queue` | `aws:ResourceTag` | `sqs:SendMessageBatch` | Yes | ALLOWED | Team / EventBridge delivery. |
| `sqs:SendMessageBatch` | `queue` | `aws:ResourceTag` | authorized as `sqs:SendMessage` | Yes | ALLOWED | Per AWS docs, batch actions are authorized by the single-operation permission. No separate SCP statement needed; listed here for parity. |
| `sqs:ReceiveMessage` | `queue` | `aws:ResourceTag` | — | Yes | ALLOWED | Team consumes DLQ messages. |
| `sqs:DeleteMessage` | `queue` | `aws:ResourceTag` | `sqs:DeleteMessageBatch` | Yes | ALLOWED | Acknowledgement. |
| `sqs:DeleteMessageBatch` | `queue` | `aws:ResourceTag` | authorized as `sqs:DeleteMessage` | Yes | ALLOWED | Batch equivalent. |
| `sqs:ChangeMessageVisibility` | `queue` | `aws:ResourceTag` | `sqs:ChangeMessageVisibilityBatch` | Yes | ALLOWED | Visibility timeout management. |
| `sqs:ChangeMessageVisibilityBatch` | `queue` | `aws:ResourceTag` | authorized as `sqs:ChangeMessageVisibility` | Yes | ALLOWED | Batch equivalent. |

### 2.6 Tagging

| Action | Resource types | Condition keys (relevant) | Batch equivalence | SCP-eligible | Protection | Notes |
|---|---|---|---|---|---|---|
| `sqs:TagQueue` | `queue` | — | — | Yes | PROTECTED | Unconditional deny on `{org}-*` queues. Prevents adding, modifying, or forging ownership tags on Konductor-managed queues. |
| `sqs:UntagQueue` | `queue` | — | — | Yes | PROTECTED | Same shape as `TagQueue`. Prevents stripping ownership tags to evade name-based enforcement. |
| `sqs:ListQueueTags` | `queue` | — | — | Yes | ALLOWED | Read-only. |

---

## 3. IAM Role Operations (trust / recovery role protection)

Service prefix: `iam`. Service Authorization Reference:
<https://docs.aws.amazon.com/service-authorization/latest/reference/list_awsidentityandaccessmanagementiam.html>

Scope: the exact trust-role ARN
`arn:aws:iam::<member-account>:role/{org}-konductor-trust-role` and the
configured recovery-role ARN. No wildcard role selectors.

| Action | Resource types | Condition keys (relevant) | Batch equivalence | SCP-eligible | Protection | Notes |
|---|---|---|---|---|---|---|
| `iam:UpdateAssumeRolePolicy` | `role` | `aws:ResourceTag`, `iam:ResourceTag` | — | Yes | PROTECTED | Rewriting the trust policy on the protected role would let a local admin open it to attacker principals. Deny on the exact role ARN. The trust role itself is NOT in the exception list — the Konductor trust role must not rewrite its own trust. |
| `iam:AttachRolePolicy` | `role` | `iam:PolicyARN`, `aws:ResourceTag` | — | Yes | PROTECTED | Attaching extra managed policies expands the role's authority. |
| `iam:DetachRolePolicy` | `role` | `iam:PolicyARN`, `aws:ResourceTag` | — | Yes | PROTECTED | Detaching could strip protective or operational policies. |
| `iam:PutRolePolicy` | `role` | `aws:ResourceTag` | — | Yes | PROTECTED | Inline-policy edits change effective permissions. |
| `iam:DeleteRolePolicy` | `role` | `aws:ResourceTag` | — | Yes | PROTECTED | Inline-policy removal. |
| `iam:PutRolePermissionsBoundary` | `role` | `iam:PolicyARN`, `aws:ResourceTag` | — | Yes | PROTECTED | Boundary changes can raise or lower ceilings. |
| `iam:CreateRole` | `role` | `aws:RequestTag`, `aws:TagKeys` | — | Yes | PROTECTED (scoped) | Deny on `arn:aws:iam::<account>:role/{org}-konductor-trust-role` and `arn:aws:iam::<account>:role/{org}-konductor-trust-role-*` so a local admin cannot recreate the role name after deletion, or create a look-alike that the SCP exception would match by name. The trust role is recreated only through the centrally configured recovery path. |
| `iam:DeleteRole` | `role` | `aws:ResourceTag` | — | Yes | PROTECTED | Deleting the protected role breaks the management path. |
| `iam:PassRole` | `role` | `iam:PassedToService`, `aws:ResourceTag` | — | Yes | PROTECTED | Passing the protected role to a service hands its authority to that service. Deny on the exact role ARN. |
| `iam:SetDefaultPolicyVersion` | `policy` | `iam:PolicyARN` | — | Yes | PROTECTED | Flipping the default version of a managed policy attached to the protected role can silently grant prior broader permissions. Deny where `iam:PolicyARN` matches any policy attached to the protected role; for simplicity the guardrail denies this action on the Konductor-owned managed policies. |

---

## 4. AWS Organizations

Service prefix: `organizations`. Service Authorization Reference:
<https://docs.aws.amazon.com/service-authorization/latest/reference/list_awsorganizations.html>

| Action | Resource types | Condition keys (relevant) | Batch equivalence | SCP-eligible | Protection | Notes |
|---|---|---|---|---|---|---|
| `organizations:LeaveOrganization` | (account) | — | — | Yes | PROTECTED | Deny at the account scope. Prevents a member account from shedding SCP guardrails by leaving the organization. This is the only Organizations action the guardrail package restricts; the central organization administrator retains all other authority. |

---

## 5. Coverage Limits — what SCPs CANNOT restrict

The following paths are documented exclusions. The guardrail package's
installation guidance calls these out; this matrix does not claim coverage.

| Path | Reason |
|---|---|
| Management-account principals | SCPs do not apply to the Organizations management account. Operations originating from the management account bypass every statement in this matrix. |
| Service-linked roles (SLRs) | AWS documentation excludes SLR sessions from SCP evaluation for actions the SLR is designed to perform. The guardrail cannot restrict an SLR acting within its documented purpose. |
| AWS service principals performing documented operations | Some service-mediated operations (e.g., support workflows, certain backup/DR services) evaluate outside SCP deny scope. Where a service-mediated path could mutate a protected resource, it is listed in the package's deployment notes. |
| Resource-based policy grants that do not require identity allow | SCPs restrict identity-based evaluation. Resource policies alone cannot exceed an SCP ceiling, but a resource policy that *grants* to a principal inside the SCP-covered account still requires identity allow — so this is not a gap in practice. Documented for completeness. |
| Root user of the management account | Same as "Management-account principals." |
| Actions not in the AWS Service Authorization Reference for SCP | If AWS ships a new action (e.g., a future `events:UpdateEventBus`), coverage is conditional on AWS adding SCP support. The matrix flags these for forward coverage. |

---

## 6. Condition-key strategy

| Condition key | Where used | Why |
|---|---|---|
| Resource ARN / name pattern | All `PROTECTED` rows | Durable anchor. Tags can be removed; names cannot. |
| `aws:PrincipalArn` | SCP exception clause on every `Deny` | Exact-match allow-list for the trust role and recovery role. No wildcards. |
| `aws:TagKeys` / `aws:RequestTag` | `CreateRole` row only | Restricts tag mutation on role creation. Tag-mutation rows on buses/queues use unconditional denies (no tag-key conditions). |
| `iam:PolicyARN` | `AttachRolePolicy`, `DetachRolePolicy`, `SetDefaultPolicyVersion`, `PutRolePermissionsBoundary` | Narrows policy-manipulation denies to the Konductor-owned policies. |
| `iam:PassedToService` | `iam:PassRole` (forward coverage) | Available if a future revision needs to scope `PassRole` to specific services. The current template denies `PassRole` on the exact role ARN regardless of destination. |

---

## 7. References

- Amazon EventBridge — Service Authorization Reference:
  <https://docs.aws.amazon.com/service-authorization/latest/reference/list_amazoneventbridge.html>
- Amazon SQS — Service Authorization Reference:
  <https://docs.aws.amazon.com/service-authorization/latest/reference/list_amazonsqs.html>
- AWS IAM — Service Authorization Reference:
  <https://docs.aws.amazon.com/service-authorization/latest/reference/list_awsidentityandaccessmanagementiam.html>
- AWS Organizations — Service Authorization Reference:
  <https://docs.aws.amazon.com/service-authorization/latest/reference/list_awsorganizations.html>
- AWS Organizations — Service control policies (evaluation logic, exclusions):
  <https://docs.aws.amazon.com/organizations/latest/userguide/orgs_manage_policies_scps.html>
- IAM — Batch action authorization semantics (SQS example):
  <https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-api-permissions-reference.html>
