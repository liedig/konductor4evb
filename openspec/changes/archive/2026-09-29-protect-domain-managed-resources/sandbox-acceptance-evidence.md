# Sandbox Acceptance Evidence: SCP Guardrail Enforcement

**Run date:** 2026-09-29 (UTC)
**Authorization:** sandbox run authorized by the organization administrator and sandbox account owner in the closing review of this change (conversation of record, 2026-09-29), scoped to sandbox account `210987654321` only; the one-time SCP policy-type enablement at the organization root was separately authorized in the same review.
**Procedure:** `sandbox-acceptance.md` (this directory), sections 0–7.

## Environment

| Field | Value |
|---|---|
| Organization | `o-lipnxj8tgf` (FeatureSet ALL) |
| Management / platform account | `444433332222` (profile `sliedig-Admin`, assumed-role `Admin`) |
| Sandbox member account | `210987654321` "Stephen Liedig" (profile `demo`, assumed-role `Admin`) |
| Region | `ap-southeast-2` (all calls pinned) |
| Org prefix | `acme` |
| Trust role | `arn:aws:iam::210987654321:role/acme-konductor-trust-role` |
| Recovery role (created for this run) | `arn:aws:iam::210987654321:role/acme-konductor-recovery-role` |
| Team fixture role (created, deleted after) | `acme-sandbox-team-role` |
| Stack | `konductor-guardrails` (CloudFormation, management account) |

## Policy versions under test

| Policy | ID | Content sha256 (run 1) | Content sha256 (run 2, after fix) |
|---|---|---|---|
| KonductorOrganizationProtection | `p-5gt0p4yj` | `fa244054…0460` | unchanged |
| KonductorProtectedIdentities | `p-i2v9m0jj` | `3a018017…b46f` | unchanged |
| KonductorProtectedResources | `p-ufevrdqf` | `abfbc3ea…ce09` | `738f33ca…9663` |

No managed policy versions were modified by any test case. Managed-policy version capture (procedure 1.8): not applicable — the matrix touches no customer managed policy.

## Run 1 (policy hash `abfbc3ea…`) — FAIL, root-caused

N1 (`events:DeleteEventBus`), N2 (`events:CreateEventBus` reserved name), and N4 (`sqs:DeleteQueue`) executed by the sandbox administrator were **ALLOWED** (00:51:46–00:51:50Z). N3, N5, N6, N7a, N7b errored with `ResourceNotFound`/`NonExistentQueue` — secondary effects of the N1/N4 deletions, not SCP denies.

**Root cause:** every resource selector in `KonductorProtectedResources` embedded `${AWS::AccountId}` (`guardrail-stack.yaml`). The pseudo-parameter resolves to the *stack deployer's* account (the organization management account `444433332222`), while the SCP attaches to the member account whose resource ARNs carry `210987654321`. The selectors matched nothing, so no deny fired anywhere. SCPs are additionally never evaluated for management-account principals, so the deployed statements were inert in both directions.

The local fixture suite did not catch this because its intrinsic resolver maps `${AWS::AccountId}` → `*` ("SCPs apply per-account"), normalizing the bug away — exactly the gap design decision 5 predicts for fixture-only verification.

**Fix (this run, left uncommitted in the working tree for review):**

- `guardrail-stack.yaml`: the four events/sqs selectors now use a wildcard account field (`arn:aws:events:*:*:event-bus/${Organization}-*`, `arn:aws:sqs:*:*:${Organization}-*`); attachment targets already scope which accounts a policy covers. A why-comment marks the non-obvious choice.
- `lib/konductor-common/tests/guardrail_fixtures.rs`: new regression test `resource_selectors_are_account_agnostic` asserts on the raw template text that no events/sqs selector may couple the account field to `${AWS::AccountId}`. Full fixture suite: 5/5 pass.

Fixture damage from run 1 (acceptance bus/DLQ deleted, `acme-rogue-bus` created) was contained to disposable fixtures; live `billing`/`sales` domain resources were never targeted. Fixtures were recreated before run 2 and the rogue bus deleted.

## Run 2 (policy hash `738f33ca…`) — PASS

### Positive cases (expected ALLOW)

| Case | Principal | Call | Observed |
|---|---|---|---|
| P1 | team | `events:PutRule` on `acme-sandbox-acceptance-event-bus` | ALLOW (00:46:44Z) |
| P2 | team | `events:DeleteRule` own rule | ALLOW (00:46:45Z) |
| P3 | team | `sqs:ReceiveMessage` integration DLQ | ALLOW (00:47:04Z) |
| P4 | team | `sqs:DeleteMessage` (fresh receipt handle) | ALLOW (00:47:33Z) |
| P5 | team | `sqs:ChangeMessageVisibility` | ALLOW (00:47:08Z) |
| P6 | team | `sqs:GetQueueAttributes` | ALLOW (00:47:09Z) |
| P7 | trust-role chain | `events:CreateEventBus` + `sqs:CreateQueue` on `acme-acceptance-p7-*` | ALLOW (00:50:14Z) |
| P8 | trust-role chain | `events:DeleteEventBus` + `sqs:DeleteQueue` on P7 fixtures (compensation shape) | ALLOW (00:50:14Z) |
| P9 | trust-role chain | `events:Tag/UntagResource` on fixture bus; `sqs:Tag/UntagQueue` on fixture DLQ | ALLOW (00:50:14Z) |
| P10a | recovery | `iam:GetRole` on the trust role | ALLOW (02:00:06Z) |

P7–P9 ran through the production call chain: harness Lambda `konductor-acceptance-harness` executing as `konductor-function-role` in the platform account, assuming `acme-konductor-trust-role` in the sandbox (session `acceptance-p7-p9`, principal ARN `…:role/acme-konductor-trust-role`). This proves the `aws:PrincipalArn` exception matches the exact role ARN for assumed-role sessions. The harness function was deleted after the run.

P10b (recovery probe, expected *IAM deny without SCP attribution*): `iam:CreateRole` on `acme-konductor-trust-role-clone-probe` failed with "no identity-based policy allows" and **no** SCP text (02:00:08Z) — proving `DenyTrustRoleRecreation` exempts the recovery principal as designed, and surfacing finding F3 below.

### Negative cases (expected DENY, all under the sandbox account admin)

| Case | Call | Observed |
|---|---|---|
| N1 | `events:DeleteEventBus` fixture bus | SCP deny (01:58:18Z) |
| N2 | `events:CreateEventBus` `acme-rogue-bus` | SCP deny (01:58:20Z) |
| N3 | `events:PutPermission` on fixture bus | SCP deny (01:58:21Z) |
| N4 | `sqs:DeleteQueue` integration DLQ | SCP deny (01:58:22Z) |
| N5 | `sqs:PurgeQueue` integration DLQ | SCP deny (01:58:24Z) |
| N6 | `sqs:SetQueueAttributes` integration DLQ | SCP deny (01:58:25Z) |
| N7a | `events:UntagResource` fixture bus | SCP deny (01:58:26Z) |
| N7b | `sqs:UntagQueue` integration DLQ | SCP deny (01:58:27Z) |
| N8 | `iam:UpdateAssumeRolePolicy` on trust role | SCP deny (01:58:43Z) |
| N9 | `iam:AttachRolePolicy` on trust role | SCP deny (01:58:45Z) |
| N10 | `iam:DeleteRole` on trust role | SCP deny (01:58:47Z) |
| N11 | `iam:PassRole` trust role to Lambda `CreateFunction` | SCP deny (01:58:49Z) |
| N12 | `iam:CreateRole` `acme-konductor-trust-role-evil` | SCP deny (01:58:50Z) |
| N13 | `organizations:LeaveOrganization` | SCP deny (01:58:52Z) |

Every deny message names the enforcing policy, e.g. N10: "…with an explicit deny in a service control policy: arn:aws:organizations::444433332222:policy/o-lipnxj8tgf/service_control_policy/p-i2v9m0jj". N12's second half (look-alike role calls `events:DeleteEventBus`) is not applicable: look-alike creation is itself denied, so no look-alike exists to test with.

### Negative controls (expected ALLOW — selector hygiene)

| Case | Call | Observed |
|---|---|---|
| C1 | `events:DeleteEventBus` non-`acme-*` bus | ALLOW (01:59:04Z) |
| C2 | `sqs:DeleteQueue` non-`acme-*` queue | ALLOW (01:59:05Z) |
| C3 | `iam:PutRolePolicy` on non-trust role | ALLOW (01:59:39Z) |
| C4 | team `events:PutRule`/`DeleteRule` after the negatives | ALLOW (01:59:42–43Z) |

### Harness artifacts (recorded, not counted)

- P1/P2 first attempt: exit 127, case-runner treated the command string as an executable name. Fixed runner, re-ran.
- P4 first attempt: receipt handle read while the message was still in its visibility timeout (`'None'` → `ReceiptHandleIsInvalid`). Re-seeded, re-ran.
- C3 first attempt: trust policy document passed where a permissions policy was required (`MalformedPolicyDocument`). Re-ran with a minimal permissions document.

## Findings

- **F1 (fixed in this run):** resource selectors coupled to `${AWS::AccountId}` — see Run 1. Template fix plus regression test `resource_selectors_are_account_agnostic`.
- **F2 (docs note):** `DenyPolicyVersionRollback` in the identities policy targets `arn:aws:iam::<deployer-account>:policy/acme-konductor-*`. SCPs are never evaluated for management-account principals, and IAM customer managed policies are account-local, so the statement is inert wherever the platform account is the management account (as here). Harmless defense-in-depth; the installation guidance should say the statement only has effect when the platform account is a member account.
- **F3 (installation-guidance amendment):** the recovery role's IAM policy needs `iam:CreateRole`/`iam:DeleteRole` on the trust-role name pattern (`{org}-konductor-trust-role*`) in addition to the maintenance actions, or the recreation exemption proven by P10b cannot be exercised. The SCP side is already correct; this is a bootstrap-grant gap. The repo deliberately treats the recovery role's shape as an installation input, so the guidance in `docs/operations/guardrails.md` should state the minimum grant set.
- **F4 (process validation):** the fixture harness normalized `${AWS::AccountId}` to `*`, which is why design decision 5's live-run requirement exists. The naive fixture evaluator keeps its value for regression coverage but must never be cited as enforcement evidence.

## Cleanup record (procedure section 6)

1. All three SCPs detached from `210987654321` (02:0x UTC); post-cleanup `list-policies-for-target` shows only `p-FullAWSAccess` (attached by the policy-type enablement).
2. Deleted: acceptance bus/DLQ fixtures, `acme-rogue-bus`, control bus (deleted as C1), control queue (deleted as C2), control role, team fixture role, harness Lambda. Post-cleanup listing shows no `acme-sandbox-*` / `acme-rogue-*` residue.
3. No managed policy default versions were modified; nothing to restore.
4. Trust role and recovery role left in place (they predate/postdate the test run by design).
5. The `konductor-guardrails` stack remains deployed with its three policies **unattached** — the package delivered but not armed. Re-attachment is a separately authorized step. The SCP policy type remains enabled at the org root (reverting is optional; with no custom SCPs attached it changes nothing).

## Acceptance statement

Per `sandbox-acceptance.md` section 7: **SCP enforcement verified in sandbox for policy content hash `738f33ca08263a4167870373771f9e8285b235f94b09304398c87e6467339663` (KonductorProtectedResources), `3a018017…b46f` (KonductorProtectedIdentities), and `fa244054…0460` (KonductorOrganizationProtection) on 2026-09-29.**

This statement does not cover: management-account principals, service-linked roles, organization administrators, accounts the SCPs are not attached to, or resource-policy-only service paths (procedure section 0). Production rollout requires the staged attachment sequence in `docs/operations/guardrails.md` plus explicit organization-admin approval per stage.

Raw CLI transcripts are retained alongside this log's source (`/tmp/scp-acceptance/transcripts.log` on the executing machine at run time); the table above carries the required per-case fields (timestamp, principal, call, expected/observed, error class), and policy IDs plus content hashes are recorded above for both runs.
