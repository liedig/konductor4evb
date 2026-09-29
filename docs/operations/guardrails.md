# Organization guardrails

[Documentation home](../README.md)

This is an optional organization-admin operation. It is separate from installing Konductor and from granting cross-account trust.

> **D09 resolved:** the account-selector defect recorded as
> [D09](../reference/documentation-discrepancies.md#d09) was fixed
> (account-agnostic selectors with a raw-template regression test) and
> verified by the authorized
> [sandbox acceptance run](../../openspec/changes/archive/2026-09-29-protect-domain-managed-resources/sandbox-acceptance-evidence.md)
> on 2026-09-29.

## Authorization boundary

> **Warning: Separate authorization required.** This phase changes AWS
> Organizations policy attachments. It requires organization-admin
> credentials, explicit authorization from your cloud governance process,
> and completed [sandbox verification](#verification--local-fixtures-first-then-authorized-sandbox) before any production
> attachment. Skipping this phase leaves Konductor fully functional but
> without the deny ceiling that stops domain-account administrators from
> modifying Konductor-managed resources. The trust stack alone grants
> Konductor access but constrains no one else.

The trust stack (Phase 3) is an *allow grant*: it lets Konductor manage
`{org}-*` resources in the domain account. It does not prevent a
domain-account administrator from deleting or reconfiguring those
resources. The guardrail stack (`guardrail-stack.yaml`) closes that gap
with three service control policies (SCPs)—a **deny ceiling** over every
principal in the covered accounts. SCPs restrict; they never grant. Team
roles still need their ordinary IAM allows, and Konductor still needs the
trust role.

### Deployment order

Deploy this phase after the trust stack and after meeting the
[prerequisites](#prerequisites). This is a separate deployment step, not part
of `make deploy-*`.

### Required inputs

| Input | Description |
|---|---|
| `Organization` | The same short identifier used in Phases 2–3 (for example, `acme`) |
| `KonductorTrustRoleArn` | Exact ARN of the trust role from Phase 3 (for example, `arn:aws:iam::<domain-account>:role/acme-konductor-trust-role`). Exact match—no wildcards. |
| `RecoveryRoleArn` | Exact ARN of a centrally controlled recovery/maintenance role. This role is exempt from every SCP deny and is the only in-model maintenance path for the trust role. |
| `TargetOuIds` and/or `TargetAccountIds` | Organizational units (OUs), individual account IDs, or both for SCP attachment. Specify at least one target. |

### Prerequisites

1. **Recovery role bootstrapped and tested first.** Create the recovery
   role in the target accounts, confirm its central principal can assume
   it, and confirm it can read the trust role—**before** attaching any
   SCP. Attaching guardrails without a working recovery path risks lockout
   (the ultimate fallback is central SCP detachment by the organization
   administrator).

   Exemption is not a grant. The SCPs exempt the recovery role's exact ARN
   from the deny ceiling, but the role's own identity policy must still
   allow the maintenance **and recreation** set on the trust role, or the
   exemption cannot be exercised. Minimum grant set on
   `arn:aws:iam::<account>:role/{org}-konductor-trust-role*`:
   `iam:GetRole`, `iam:CreateRole`, `iam:DeleteRole`, `iam:UpdateRole`,
   `iam:UpdateAssumeRolePolicy`, `iam:TagRole`, `iam:UntagRole`,
   `iam:PutRolePolicy`, `iam:DeleteRolePolicy`, `iam:AttachRolePolicy`,
   `iam:DetachRolePolicy`. (Sandbox evidence: a recovery role granted only
   read and maintenance actions received a plain IAM deny—correctly without
   SCP attribution—when recreating the trust role.)
2. **Resource inventory.** Confirm existing `{org}-*` buses, queues, and
   roles in the target accounts match the supported naming selectors.
   Resources outside the selector patterns are not covered.

### Deploy (organization admin credentials)

```bash
aws cloudformation deploy \
  --template-file guardrail-stack.yaml \
  --stack-name konductor-guardrails \
  --parameter-overrides \
    Organization=acme \
    KonductorTrustRoleArn=arn:aws:iam::<domain-account-id>:role/acme-konductor-trust-role \
    RecoveryRoleArn=arn:aws:iam::<domain-account-id>:role/<recovery-role-name> \
    TargetAccountIds=<sandbox-account-id> \
  --region <region>
```

### Verification — local fixtures first, then authorized sandbox

1. **Local verification**: run the repository's policy/template fixture
   tests. These prove the SCP statements match the enforcement matrix —
   they do **not** prove live enforcement.
2. **Authorized sandbox verification**: attach the SCPs to an isolated
   sandbox account/OU first and run the full acceptance procedure in
   `openspec/changes/archive/2026-09-29-protect-domain-managed-resources/sandbox-acceptance.md`
   (positive team operations, denied administrator operations, Konductor
   provisioning/compensation, recovery path). Retain the recorded
   evidence: policy versions, targets, principals, observed AWS outcomes.
3. **Staged production attachment**: only after a documented sandbox pass,
   attach to production OUs/accounts in stages, with explicit
   organization-admin approval at each stage.

> **Do not claim protection without sandbox verification.** A deployed
> stack and passing fixtures show the policies exist and are well-formed;
> only the sandbox run demonstrates that AWS actually denies the protected
> operations. Until then, describe the installation as "guardrails
> deployed, enforcement unverified."

### Rollback

If a guardrail breaks a legitimate operation: **detach** the affected SCP
from the affected target → **repair** via the recovery role (fix policy
content or exception ARNs) → **re-attach** the validated version. Do not
delete protected resources as a rollback strategy. Record the temporary
protection gap while detached.

### Coverage limits (read before claiming protection)

- SCPs do **not** apply to the AWS Organizations **management account** —
  principals operating there bypass every statement.
- The version-rollback deny (`DenyPolicyVersionRollback`, covering
  `{org}-konductor-*` customer managed policies) takes effect only when the
  platform account is a **member** account. The statement binds
  `AWS::AccountId` at deploy time and SCPs are never evaluated in the
  management account, so in a co-located deployment (platform account is
  the management account) it is inert; in a split deployment it binds the
  stack deployer's account rather than the platform account. Either way,
  the real protection for those managed policies is the IAM grant surface
  of the account that owns them.
- **Service-linked roles** are exempt from SCP evaluation for actions they
  are designed to perform.
- Organization administrators remain trusted and retain full control of
  attachments.
- Console links in the web UI are navigation conveniences, not an
  authorization mechanism—they do not enforce read-only (or any) AWS
  access.

## Ownership and policy scope

**Two-account resource map.** Konductor-managed infrastructure splits across the two account types:

| Account | Resources | Console-linkable from domain page |
|---|---|---|
| Platform account | Platform event bus, schema registry, subscription DLQ, CloudWatch dashboard (all `konductor-*` or `{org}-{env}-{domain}-*` in the platform account) | No—text-only, even in a co-located deployment |
| Domain account | Application event bus, integration DLQ (`{org}-{env}-{domain}-*` in the owner's account) | Yes, when the identifier parses to the domain's `ownerAccountId` |

**IAM grants vs SCP ceilings.** These are independent layers and both are required:

- **IAM grants** (identity-based policies, the trust stack, team roles) say what a principal *can* do. Without a grant, the action is denied regardless of SCPs.
- **SCP ceilings** say what *any* principal in the covered account may do at most. An SCP `Deny` beats every identity-based `Allow`, including `AdministratorAccess` on the member account.

The guardrail SCPs protect exactly four surfaces on `{org}-*` resources: EventBridge bus lifecycle/policy/tagging, SQS queue lifecycle/configuration/purge/tagging, IAM manipulation of the exact trust-role and recovery-role ARNs (no wildcards), and `organizations:LeaveOrganization`. Team-owned operations—integration rule/target management on the app bus, DLQ message receive/delete/visibility—are deliberately **not** denied; team IAM continues to gate them. The full action-by-action contract lives in the change's `enforcement-matrix.md`.

## Design and acceptance evidence

- [Guardrail decision](../decisions/protect-domain-managed-resources.md)
- [Enforcement matrix](../../openspec/changes/archive/2026-09-29-protect-domain-managed-resources/enforcement-matrix.md)
- [Authorized sandbox procedure](../../openspec/changes/archive/2026-09-29-protect-domain-managed-resources/sandbox-acceptance.md)
- [Sandbox acceptance evidence](../../openspec/changes/archive/2026-09-29-protect-domain-managed-resources/sandbox-acceptance-evidence.md)
- [Acceptance tasks](../../openspec/changes/archive/2026-09-29-protect-domain-managed-resources/tasks.md) (complete)

The account-selector discrepancy (D09) is resolved and a sandbox pass is
recorded in the acceptance evidence (2026-09-29; `KonductorProtectedResources`
content hash `738f33ca…9663`).
