# Cloud Correctness Evidence — DynamoDB grant-engine spike

> Spike artifact. Produced by running the frozen scenario suite against `DynamoGrantAdapter` on the
> provisioned sandbox tables. Complements `local-correctness.md` (in-memory) by re-verifying every
> scenario against real DynamoDB strongly-consistent reads, the `grant_id-index` GSI revocation
> path, and synchronous subscription reconciliation.

## 1. Execution environment

- Adapter: `DynamoGrantAdapter` (real DynamoDB, `ap-southeast-2`, sandbox account `444433332222`, default profile)
- Tables: `nexus-spike-auth-grant` (pk/sk + `grant_id-index` GSI), `nexus-spike-auth-sub` (subscription_id)
- Billing: on-demand (`PAY_PER_REQUEST`); tagged `nexus-spike=evaluate-authorization-engine`, `managed-by=droid-spike`
- Read consistency: `StronglyConsistent` on every decision path; GSI used for revocation candidate lookup only
- Runner: `ScenarioRunner` over `DynamoGrantAdapter`; per-scenario `seed()` clears both tables before seeding (parity with the in-memory adapter's reset semantics)
- Frozen manifest SHA-256: `ec4efb25c034159aafb041bc50061961dc0140c10380ab794ce9067547b441b0`
- Test command: `AWS_PROFILE=default AWS_REGION=ap-southeast-2 cargo test -p auth-engine-spike --test dynamo_correctness -- --ignored --nocapture`

## 2. Correctness results

| Suite | Scenarios | Steps | Result |
|---|---|---|---|
| `all_scenarios_pass_against_dynamo_adapter` | 13/13 | 34/34 | pass |
| `s10_revocation_reconciles_subscriptions_in_dynamo` | S10 | 2/2 | pass |

- All 13 frozen scenarios pass against real DynamoDB; 0 failing steps.
- Wall time for the full suite: ~17.5 s (includes per-scenario table clear + seed + schema registration + decisions).

## 3. Cloud-verified behaviors

| Behavior | Scenarios | Cloud verification |
|---|---|---|
| Global platform roles (admin/auditor) | S01, S02 | strongly-consistent read not required (authorizer-supplied role); decision composes correctly |
| Cognito group binding / publisher / subscriber | S03, S04, S05 | GetItem on `PRINCIPAL#...` binding keys with `ConsistentRead=true` |
| Producer-domain + exact-schema entitlements | S06, S07 | Query `CONSUMER#...` with `begins_with(sk, ENT#PRODUCER#...)`, coverage computed over active entitlements |
| Producer growth after grant | S08 | domain-wide consumer gains the new schema without mutation; exact-schema consumer does not (no entitlement expansion) |
| Additive overlapping grants + removal | S09 | union coverage preserved; removing one of two overlapping grants leaves coverage via the other |
| Revocation + subscription reconciliation | S10 | `grant_id-index` Query -> UpdateItem `state=revoked`; reconcile scans active subscriptions and disables those without remaining coverage (`sub-002` -> `disabled`), preserves overlapping (`sub-003` -> `active`) |
| Cognito-group / AWS-account parity | S11 | same decision for cognito-group and aws-account subscribers; assumed-role ARN is audit-only; forged issuer -> `deny:untrusted-issuer` |
| Non-disclosure | S12 | unauthorized existing schema -> `deny-as-unknown`, indistinguishable from unknown schema; list filtering omits unauthorized schemas |
| Asynchronous handling | S06 (enqueue/delayed), S13 (revocation-before-consume, replay, incomplete, tampered) | deterministic denials acknowledged, not retried; `replay_of`/`seen_commands` dedup; `missing_field`/`tampered` early-deny |

## 4. Asynchronous-handling confirmation (task 5.3)

The frozen suite exercises every asynchronous command variant against the cloud adapter:

- **enqueue / delayed handling** (S06): the command is accepted and the decision is evaluated against current grants at consume time.
- **revocation-before-consume** (S13): a grant revoked between enqueue and consume produces `deny` at consume; the denial is acknowledged, not retried.
- **replay** (S13): a duplicate `command_id` (or `replay_of` referencing a seen command) returns `deny:duplicate-command`.
- **incomplete context** (S13): `missing_field` set -> `deny:incomplete-principal-context`.
- **tampered context** (S13): `tampered=true` -> `deny:tampered-principal-context`.

All deterministic denials are returned exactly once (SDK retries disabled on decision reads, `AWS_MAX_ATTEMPTS=1`), confirming the contract that deterministic denials are acknowledged rather than retried.

## 5. Mandatory behavior status (cloud)

- **Correctness**: all 13 frozen scenarios pass against `DynamoGrantAdapter` on real DynamoDB.
- **Non-disclosure**: S12a/S08c return `deny-as-unknown` (byte-for-byte indistinguishable from the S12b unknown-schema control); S12c list filtering omits unauthorized schemas. Verified in-cloud.
- **Revocation contract**: S10 confirms grant state flip, subscription reconciliation, overlapping preservation, and subsequent denied decision — all against real DynamoDB.
- **Decision**: cloud correctness and non-disclosure behavior are satisfied. Latency, cost, revocation-propagation timing, and operational exercises are addressed in `bench-latency.md`, `cost-worksheet.md`, and the operational-exercises evidence.
