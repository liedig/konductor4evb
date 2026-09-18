## Context

See `proposal.md` for motivation and scope.

The domain detail page passes every resource identifier through `consoleLinkFor` without checking account ownership. The provisioning workflow's `EmitEnvSucceeded` output contains four platform resources (platform bus, registry, subscription DLQ, dashboard) and two domain-account resources (application bus, integration DLQ). Both DLQ fields currently contain queue URLs despite their `Arn` suffixes.

`trust-stack.yaml` trusts the platform execution, provisioning, and retagging roles. Its attached policies authorize management of organization-prefixed buses, queues, and legacy bus-to-bus roles. It neither governs other local principals nor installs organization guardrails. The earlier trust-role-recreation design explicitly excluded SCPs and permissions boundaries. This change adds that missing protection rather than treating the existing trust policy as a deny boundary.

Existing topology requirements deliberately let application teams manage integration rules on their application bus. Broad statements that teams have "no control-plane access" conflict with that behavior; this design preserves rule ownership while protecting the underlying infrastructure.

## Goals / Non-Goals

**Goals:**
- Enforce the infrastructure ownership boundary against member-account administrators, not just application roles with curated permissions.
- Keep protected-resource operations and role exceptions small, explicit, and independently testable.
- Keep presentation, application authorization, and organization enforcement distinct.

**Non-Goals:**
- Protect against organization administrators, management-account principals outside SCP coverage, or compromise of the trusted platform itself.
- Replace application IAM grants, introduce a general landing-zone manager, or attach SCPs from runtime handlers.
- Change publishing, schema compatibility, metadata composition, subscription routing, or the existing grant engine.
- Remove the unused bus-to-bus role-management permissions as unrelated cleanup.

## Decisions

### 1. Classify resource purpose first, verify account identity second

Use the known resource fields to separate "Platform-managed resources" from "Resources in your account." Both groups remain explicitly described as Konductor-managed infrastructure; account location does not imply configuration ownership.

Only application-bus and integration-DLQ fields are link-eligible, and only when a recognized ARN or queue URL identifies the domain's `ownerAccountId`. Platform fields remain text even in a co-located deployment. Missing, malformed, unknown, or inconsistent values fall back to text without inventing a console URL. Preserve the existing API payload and region fallback for compatible resource parsing. Do not infer console access from a Cognito group or present the link as a read-only AWS session.

Alternative rejected: account comparison alone, which misclassifies co-located platform resources. Removing all links would unnecessarily harm valid domain-account navigation.

### 2. Deliver centrally administered guardrails as a separate package

Provide an organization-admin CloudFormation/policy package with explicit target member accounts or OUs, protected organization/resource selectors, exact platform trust-role identity, and recovery configuration. Keep it separate from `konductor-core-stack.yaml` and `trust-stack.yaml` so ordinary deployments do not gain organization-wide authority.

SCPs are deny ceilings, not grants. Team roles still require ordinary IAM allows. A permissions boundary on the Konductor role or a resource-policy allow is not a substitute for an SCP protecting against local administrators.

The package must document management-account and service-linked-role exclusions. Prevent a domain administrator from leaving the organization to shed the guardrail where Organizations supports that operation. Central organization administrators retain authority over attachments and recovery.

### 3. Protect exact resource operations, not every operation on a bus

Before policy authoring, produce an action/resource/condition matrix checked against current AWS authorization references. The required outcomes are fixed:

| Surface | Domain-team behavior | Protected behavior |
|---|---|---|
| Application event bus | Read/describe; existing permitted event operations; manage own rules and targets | Bus creation in reserved namespaces, deletion, configuration changes, policy replacement/removal, protected tagging |
| Integration DLQ | Read attributes/URL; receive, delete acknowledged messages, change visibility, including supported batch equivalents | Queue creation in reserved namespaces, deletion, purge, attribute/policy/redrive-policy changes, protected tagging |
| Integration rules | Create/update/delete/enable/disable own rules and attach/remove own targets | No access to platform subscription rules; no new infrastructure-management permission |
| Trust and recovery identities | No local mutation or passing | Trust/policy changes, attachment/version manipulation, deletion/recreation, ownership-tag or boundary changes, and indirect local escalation |

Use reserved resource ARN/name selectors as the durable protection anchor for current naming patterns, with tags only where supported and necessary. Do not use a removable tag as the sole identity of a protected resource. Scope narrowly enough not to deny unrelated application infrastructure sharing the organization prefix. Existing resources outside supported selectors must be inventoried and covered explicitly before the deployment is called protected.

Normal queue consumption excludes purge and bulk redrive by default; existing event publishing permissions are not expanded. Document service-side queue-policy needs for integration-rule DLQ delivery so teams are not forced to acquire `SetQueueAttributes` merely to use the queue. If additional trusted setup is needed, limit it to that integration path and test it.

Alternative rejected: blanket `events:*`/`sqs:*` denial, which prevents intended integration work and message handling.

### 4. Protect the role exception as strongly as the resources

For resource management, exempt the exact application-account Konductor trust-role ARN, not a wildcard such as `*konductor*`. Its current trust policy still restricts callers to the three platform roles.

Local administrators must not be able to rewrite that role's trust, attach or edit permission policies, alter referenced managed-policy versions, recreate its name, pass it to another service, or acquire equivalent authority through an indirectly controlled deployment role. Exception identity is evaluated using AWS principal semantics; assumed-role sessions must be tested explicitly.

Role maintenance is distinct from ordinary resource management. Use an explicitly configured centrally trusted recovery/deployment role, bootstrap it before enforcement, and protect it against local mutation and assumption. Do not let the ordinary Konductor trust role rewrite its own trust policy. Central SCP maintenance remains the ultimate recovery path. Exact deployment account IDs, OU IDs, and recovery-role ARNs are installation inputs, not repository secrets or hard-coded examples of customer infrastructure.

### 5. Separate local assurance from authorized enforcement evidence

Add table-driven policy fixtures and template assertions alongside existing infrastructure tests. They detect missing actions, overly broad exceptions, selector bypasses, and regressions, but they are not a complete AWS IAM evaluator.

Prepare a repeatable sandbox acceptance procedure covering positive application operations, denied local-administrator operations, protected-role escalation attempts, and successful Konductor provisioning/compensation/retagging. An authorized AWS run is required before claiming deployed enforcement. No fake "SCP active" badge or success state based only on the UI or presence of a trust role.

## Risks / Trade-offs

- [Unsupported resource or condition keys silently leave gaps] -> Verify every action, including batch authorization semantics, against current AWS documentation and live sandbox results before rollout.
- [Overbroad ARN selectors break unrelated workloads] -> Inventory existing resources and test negative controls for unrelated buses, queues, IAM roles, and team integration rules.
- [Tag removal or reserved-name recreation bypasses ownership] -> Protect reserved namespaces and ownership-tag mutation; test absent and forged tags as well as delete/recreate attempts.
- [Policy attachment locks out provisioning or trust-stack updates] -> Bootstrap and verify central recovery first; exercise normal provisioning, saga rollback, retagging, and authorized trust-role upgrades under enforcement.
- [An exempt service path changes protected resources] -> Document SCP exclusions, avoid wildcard service exceptions, and examine service-mediated paths in the sandbox matrix. Do not claim coverage of service-linked-role actions that SCPs cannot restrict.
- [Domain teams cannot wire integration DLQs without changing policy] -> Verify EventBridge-to-SQS authorization and supply narrowly scoped trusted setup rather than granting teams queue-policy administration.
- [A member account is outside the intended organization target] -> Treat attachment verification and account inventory as explicit installation prerequisites, not inferred runtime protection.

## Migration Plan

1. Inventory member accounts, resources, trust policies, team permissions, existing SCPs, and supported naming patterns without mutating services.
2. Complete local UI/policy/template tests and prepare the positive/negative AWS acceptance matrix.
3. With separate authorization, bootstrap and test centrally controlled recovery and attach guardrails in an isolated member-account sandbox.
4. Run application integration and Konductor lifecycle tests; retain observed outcomes and policy versions. Do not treat an IAM simulation as a replacement for live SCP enforcement.
5. Deploy the UI independently. Roll out the guardrail to approved account/OU targets in stages after explicit organization-admin approval.
6. For recovery, centrally narrow or detach the affected policy, repair through the protected maintenance path, and reapply the validated version. Record the temporary protection gap; do not delete resources or rewrite domain history.

## Open Questions

- Which customer member-account/OU targets and centrally controlled recovery-role ARNs will be supplied at installation? These are deployment inputs; they do not change the enforcement model.
