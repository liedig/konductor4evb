# Sandbox Acceptance Procedure: SCP Guardrail Enforcement

This document is the repeatable procedure for validating that the
`guardrail-stack.yaml` SCPs actually enforce the documented boundary in a
live AWS Organization. It is the evidence gate called out in the design
(decision 5): local fixtures and template assertions are necessary but not
sufficient — only an authorized run against a real organization proves
deployed enforcement.

> **Authorization required.** Running this procedure performs real AWS API
> calls against a real AWS Organization. Obtain explicit written
> authorization from the organization administrator and the sandbox account
> owner before executing any step. Record the authorization reference in the
> evidence log. Do not run this procedure against production OUs or
> accounts.

---

## 0. Scope and Explicit Exclusions

This procedure validates SCP enforcement **only** within the documented SCP
coverage model. It does **not** test, and the guardrails do **not** claim
protection for, the following paths:

1. **Management-account principals.** SCPs do not apply to the AWS
   Organizations management account. Any principal operating from the
   management account bypasses every statement in `guardrail-stack.yaml`.
   This procedure performs no management-account test cases, and a "pass"
   result must never be read as covering management-account paths.
2. **Service-linked roles (SLRs).** Per AWS documentation, SLR sessions are
   exempt from SCP evaluation for actions the SLR is designed to perform.
   This procedure does not test SLR behavior and the guardrail does not
   claim to restrict it.
3. **Organization administrator principals.** The central organization
   administrator retains full authority over SCP attachment, detachment,
   and content. They are a trusted principal by design, not a bypass to be
   closed.
4. **Resource-based-policy-only paths.** SCPs restrict identity-based
   evaluation. Resource policies cannot exceed an SCP ceiling, but this
   procedure does not attempt to enumerate service-mediated paths (support
   workflows, backup/DR services) that may evaluate outside SCP deny scope.

---

## 1. Prerequisites

Before executing any test case, verify and record each prerequisite in the
evidence log (section 5).

| # | Prerequisite | Verification |
|---|---|---|
| 1.1 | **Isolated member account (sandbox)** within the AWS Organization, dedicated to this test run. No production workloads, no shared team resources. | `aws organizations describe-account --account-id <sandbox-id>` from the org admin credentials |
| 1.2 | **Organization admin credentials** (management account or delegated administrator for Organizations) with permission to create, update, attach, and detach SCPs. | `aws organizations list-policies --filter SERVICE_CONTROL_POLICY` succeeds |
| 1.3 | **Domain account admin credentials** for the sandbox account (IAM principal with `AdministratorAccess` or equivalent inside the sandbox account). Used for the negative test cases. | `aws sts get-caller-identity` shows the sandbox account |
| 1.4 | **Application team credentials** for the sandbox account — an IAM principal holding the ordinary team IAM grants (integration-rule management on the app bus, DLQ messaging). Used for the positive test cases. | Assume-role or profile resolves to the team role |
| 1.5 | **`trust-stack.yaml` deployed in the sandbox account**, producing the `{org}-konductor-trust-role`. | `aws iam get-role --role-name <org>-konductor-trust-role` |
| 1.6 | **Recovery role bootstrapped and tested** in the sandbox account before SCP attachment (per design decision 4: recovery-first). | Assume-role from the central recovery principal succeeds; a benign read (`iam:GetRole` on the trust role) succeeds |
| 1.7 | **`guardrail-stack.yaml` deployed** from the organization admin credentials with `Organization`, `KonductorTrustRoleArn`, `RecoveryRoleArn`, and the sandbox account ID in `TargetAccountIds`. Record the three SCP policy IDs. | `aws organizations list-policies-for-target --target-id <sandbox-id> --filter SERVICE_CONTROL_POLICY` lists all three |
| 1.8 | **Policy version IDs recorded before testing.** For every managed policy the test matrix touches, capture `aws iam get-policy` / `list-policy-versions` output. Also record each SCP's current content hash (`aws organizations describe-policy`). | Evidence log populated |
| 1.9 | **Konductor-managed fixtures exist in the sandbox account** — one application event bus (`{org}-sandbox-<domain>-event-bus`) and one integration DLQ (`{org}-sandbox-<domain>-integration-dlq`), provisioned through the normal Konductor path or created via the trust role for test purposes. | `aws events describe-event-bus --name <bus>` and `aws sqs get-queue-url --queue-name <queue>` succeed |
| 1.10 | **Unrelated control fixtures exist** — a non-`{org}-*` event bus and queue, and a non-trust IAM role, to prove the SCP selectors do not deny unrelated resources. | Describe calls succeed |

Record any prerequisite that cannot be satisfied as a blocking gap; do not
proceed with partial prerequisites.

---

## 2. Test Harness Conventions

- Run every CLI command with `--region` pinned to the sandbox test region.
- Record the full command, the exit code, and the error code from the JSON
  error response (e.g., `AccessDeniedException`, `AccessDenied`) for every
  case.
- For each principal, use a distinct named AWS CLI profile
  (`sandbox-admin`, `sandbox-team`, `konductor-trust`, `recovery`,
  `org-admin`) so the principal used per case is unambiguous.
- For assumed-role sessions (trust role, recovery role), capture the
  assumed-role ARN from `aws sts get-caller-identity` — SCP exceptions
  match on `aws:PrincipalArn`, so the session ARN must be the exact
  role ARN for the exception to apply.
- Execute cases in order. Do not skip a failing case; record the failure
  and continue unless it corrupts shared fixtures (then stop and restore).

---

## 3. Positive Test Cases (expected result: ALLOW)

These cases prove the guardrails do not break the documented team surface
or the Konductor management path.

### Application team operations

| # | Case | Command shape | Expected |
|---|---|---|---|
| P1 | Application team creates an integration rule on the app bus | `events:PutRule` on `rule/{org}-*` / event-bus `{org}-sandbox-*` under the team profile | Allow |
| P2 | Application team deletes its own integration rule | `events:DeleteRule` on the rule created in P1 | Allow |
| P3 | Application team receives messages from the integration DLQ | `sqs:ReceiveMessage` on the integration DLQ | Allow |
| P4 | Application team acknowledges a message | `sqs:DeleteMessage` with the receipt handle from P3 | Allow |
| P5 | Application team changes message visibility | `sqs:ChangeMessageVisibility` on a received message | Allow |
| P6 | Application team reads queue attributes | `sqs:GetQueueAttributes` on the integration DLQ | Allow |

For P3–P5, seed the DLQ first (publish a failing event or use
`sqs:SendMessage` under the team profile — `SendMessage` is ALLOWED by the
matrix). Batch equivalents (`SendMessageBatch`, `DeleteMessageBatch`,
`ChangeMessageVisibilityBatch`) are authorized by their single-operation
permissions per AWS documentation; spot-check one batch call if the team
workload uses batches.

### Konductor trust-role lifecycle operations

Assume the trust role from the platform-side credentials the same way the
provisioning workflow does, then:

| # | Case | Command shape | Expected |
|---|---|---|---|
| P7 | Trust role provisions resources | `events:CreateEventBus` and `sqs:CreateQueue` on `{org}-acceptance-*` names | Allow (SCP exception matches exact trust-role ARN) |
| P8 | Trust role deletes resources during compensation | `events:DeleteEventBus` and `sqs:DeleteQueue` on the P7 fixtures | Allow |
| P9 | Trust role tags/untags managed resources | `events:TagResource`/`events:UntagResource` on the app bus, `sqs:TagQueue`/`sqs:UntagQueue` on the integration DLQ | Allow |

If P7–P9 fail, the SCP exception configuration is wrong (typo in
`KonductorTrustRoleArn`, session ARN mismatch, or wrong partition). Stop
and fix before running negative cases — a guardrail that blocks the
management path is a lockout, not a control.

### Recovery-role operations

| # | Case | Command shape | Expected |
|---|---|---|---|
| P10 | Recovery role performs trust-role maintenance | `iam:GetRole`, and (on a disposable clone of the trust role name pattern, never the live one) verify the recovery role can execute the maintenance actions the SCP exempts it from | Allow |

---

## 4. Negative Test Cases (expected result: DENY)

Run every case under the **domain account admin** profile unless stated
otherwise. The expected outcome is an explicit deny from SCP evaluation —
typically `AccessDeniedException` with an message referencing a service
control policy ("with an explicit deny in a service control policy").

### Managed infrastructure mutation

| # | Case | Command shape | Expected |
|---|---|---|---|
| N1 | Local admin deletes the app event bus | `events:DeleteEventBus` on `{org}-sandbox-*` | Deny |
| N2 | Local admin creates a bus with a reserved name | `events:CreateEventBus` named `{org}-rogue-bus` | Deny |
| N3 | Local admin modifies the bus resource policy | `events:PutPermission` on the app bus | Deny |
| N4 | Local admin deletes the integration DLQ | `sqs:DeleteQueue` on the integration DLQ | Deny |
| N5 | Local admin purges a queue | `sqs:PurgeQueue` on the integration DLQ | Deny |
| N6 | Local admin changes queue attributes | `sqs:SetQueueAttributes` on the integration DLQ | Deny |
| N7 | Local admin removes protected tags | `events:UntagResource` on the app bus; `sqs:UntagQueue` on the integration DLQ | Deny |

### Protected identity manipulation

| # | Case | Command shape | Expected |
|---|---|---|---|
| N8 | Local admin modifies the trust role's trust policy | `iam:UpdateAssumeRolePolicy` on `{org}-konductor-trust-role` | Deny |
| N9 | Local admin attaches a policy to the trust role | `iam:AttachRolePolicy` on the trust role | Deny |
| N10 | Local admin deletes the trust role | `iam:DeleteRole` on the trust role | Deny |
| N11 | Local admin passes the trust role to a service | `iam:PassRole` naming the trust role ARN (e.g., in a `CreateFunction` call) | Deny |
| N12 | Look-alike role attempts a protected operation | Create role `{org}-konductor-trust-role-evil` is itself denied by the recreation guard (`iam:CreateRole` on `{org}-konductor-trust-role-*`); verify the deny, then verify no existing look-alike can call `events:DeleteEventBus` on the app bus | Deny on both |
| N13 | Member account attempts to leave the organization | `organizations:LeaveOrganization` under the sandbox admin | Deny |

### Negative controls (expected ALLOW — selector hygiene)

These cases prove the SCPs do not overreach onto unrelated resources. A
deny on any of these is a selector bug, not a pass.

| # | Case | Command shape | Expected |
|---|---|---|---|
| C1 | Local admin deletes an unrelated event bus | `events:DeleteEventBus` on a non-`{org}-*` bus | Allow (team IAM permitting) |
| C2 | Local admin deletes an unrelated queue | `sqs:DeleteQueue` on a non-`{org}-*` queue | Allow (team IAM permitting) |
| C3 | Local admin mutates an unrelated role | `iam:PutRolePolicy` on a non-trust role | Allow (team IAM permitting) |
| C4 | Application team manages rules on the app bus | `events:PutRule`/`DeleteRule` under the team profile (repeat of P1/P2 shape after negatives) | Allow |

---

## 5. Evidence Capture

Maintain an evidence log (spreadsheet, or a table appended to the
authorized test-run record) with one row per executed case. Required
columns:

| Field | Content |
|---|---|
| Case ID | P1–P10, N1–N13, C1–C4 |
| Timestamp (UTC) | When the API call was made |
| Principal | Profile name and resolved ARN (`aws sts get-caller-identity`) |
| AWS API call | Service, action, and target resource ARN/name |
| Expected result | Allow / Deny |
| Observed result | Allow / Deny, plus error code and message excerpt when denied |
| SCP policy IDs + versions | The three guardrail policy IDs and their content hashes at the time of the call |
| Managed policy versions | Before/after version IDs for any managed policy touched by the case |
| Target OUs/accounts | Sandbox account ID (and OU if attached at OU level) |
| Notes | Anomalies, retries, console vs CLI differences |

Retain the raw CLI transcripts alongside the table. Evidence without
timestamps, principals, and policy versions is not acceptable — the point
of the procedure is that a reviewer can re-derive what was enforced, by
which policy version, against which identity, at what time.

---

## 6. Cleanup

After all cases are recorded:

1. **Detach the SCPs** from the sandbox account/OU
   (`aws organizations detach-policy` for each of the three policy IDs).
   Confirm with `list-policies-for-target`.
2. **Delete test resources created during the run** — P7 fixtures, seeded
   DLQ messages (purge is now permitted once detached; otherwise delete
   the queue), control fixtures, and any look-alike roles.
3. **Restore original policy versions** if any managed policy default
   version was modified during testing (`iam:SetDefaultPolicyVersion`),
   and delete any versions created for the test.
4. **Leave the trust stack and recovery role in place** unless the
   authorization scope says otherwise — they predate the test run.
5. Record the cleanup completion timestamp and the post-cleanup
   `list-policies-for-target` output showing zero guardrail attachments.

---

## 7. Acceptance Criteria

The procedure passes when:

- Every positive case (P1–P10) is allowed.
- Every negative case (N1–N13) is denied with an SCP-attributable error.
- Every control case (C1–C4) is allowed.
- The evidence log is complete per section 5.
- Cleanup completed and post-cleanup attachment state is recorded.

A pass under this procedure authorizes the statement "SCP enforcement
verified in sandbox for policy content hash `<hash>` on `<date>`." It does
not authorize claims about the management account, service-linked roles,
organization administrators, or any account the SCPs are not attached to.
Production rollout requires the staged attachment sequence in the
installation guidance plus explicit organization-admin approval — a sandbox
pass alone is not production evidence.
