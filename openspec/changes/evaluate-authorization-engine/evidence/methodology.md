# Spike Evaluation Methodology

> **Spike methodology.** Every value in this document is a spike evaluation parameter, not a settled Nexus production requirement. Targets exist only to bound the readiness rubric; they are not committed SLOs. Production SLOs are recommended from observed evidence in the readiness decision (task 7.1).

## 1. Frozen fixtures

| Item | Value |
|---|---|
| Manifest | `evidence/fixture-manifest.json` |
| Manifest version | 1.0 |
| Frozen at | 2026-07-28 |
| SHA-256 | `ec4efb25c034159aafb041bc50061961dc0140c10380ab794ce9067547b441b0` |

The manifest is frozen before any result is collected. Any change to fixtures invalidates the hash and requires re-freezing with a new version. The readiness review (task 6.4, 7.3) confirms the hash is unchanged from the one used to produce evidence.

## 2. Expected decisions (summary)

Expected decisions are defined per scenario in the frozen manifest. The summary below is derived from the manifest and is not authoritative; the manifest is.

| Scenario ID | Scenario | Expected |
|---|---|---|
| S01 | Global platform roles (platform-admin) | allow |
| S02 | Global platform roles (platform-auditor) | read=allow, admin=deny |
| S03 | Cognito group binding | allow |
| S04 | Domain-scoped publisher | bound=allow, unbound=deny |
| S05 | Domain-scoped subscriber | bound=allow, unbound=deny |
| S06 | Producer-domain entitlement | covered schemas=allow, uncovered producer=deny |
| S07 | Exact-schema entitlement | exact schema=allow, other schema same producer=deny |
| S08 | Producer growth after grant | domain-wide=new schema allow; exact-schema=new schema deny |
| S09 | Additive grants | union=allow; remove one=allow if another covers; remove all=deny |
| S10 | Revocation affects decisions and active subscriptions | sole grant revoked=deny + subscription disabled; one of two revoked=allow + subscription stays active |
| S11 | Cognito group and AWS account parity | equivalent allow across cognito-group and aws-account; assumed-role ARN same decision; forged issuer=deny |
| S12 | Unauthorized non-disclosure | unauthorized indistinguishable from unknown; list omits unauthorized |
| S13 | Asynchronous authorization | revoked-before-consume=deny (acknowledged, not retried); replay=deny; incomplete=deny; tampered=deny |

### 2.1 Representative list filtering and unknown-resource responses

- **Unknown-resource response shape** (control): a `schema:discover` request for `payments-prod::payment-nonexistent` returns the unknown-resource response (S12b).
- **Unauthorized non-disclosure**: a `schema:discover` request for `payments-prod::payment-refunded` by `risk-cons` (which has only an exact-schema grant on `payment-captured`) MUST return a response byte-for-byte indistinguishable from the unknown-resource response (S12a). No denial reason, error code, or timing channel may reveal that the schema exists.
- **List filtering**: a `schema:discover` list request for `payments-prod` by `risk-cons` returns only `payment-captured` (the covered schema). `payment-refunded` and `payment-disputed` are omitted entirely; they are not returned as `denied` entries (S12c).

## 3. Workload profiles

Two profiles are declared for latency benchmarking (task 4.1).

| Profile | Concurrency | Description |
|---|---|---|
| `sequential` | 1 in-flight decision | Measures raw evaluator latency without queuing or contention. One decision completes before the next is issued. |
| `target-throughput` | drive at declared target throughput | Sustained offered load at the provisional target throughput (Section 5) to expose queuing, throttles, and retries. Decisions are issued by a bounded client pool. |

## 4. Sample sizes

| Measurement | Principal type | Profile | Sample size |
|---|---|---|---|
| Warm decision latency | cognito-group | sequential | 200 |
| Warm decision latency | aws-account | sequential | 200 |
| Warm decision latency | cognito-group | target-throughput | 200 |
| Warm decision latency | aws-account | target-throughput | 200 |
| Cold-start-path decision latency | cognito-group | sequential | 20 cold starts |
| Cold-start-path decision latency | aws-account | sequential | 20 cold starts |
| Revocation trials | both | sequential | 30 independent trials |

- **Warm** excludes one-time fixture setup but includes the complete authorization evaluator call (subject resolution, grant lookup, entitlement coverage, decision).
- **Cold-start-path** runs in a newly initialized evaluator process and includes AWS SDK client initialization and the first strongly-consistent read against a cold connection.
- **Revocation trials** use no authorization decision cache. Each trial revokes a sole covering grant for an active subscription and measures decision invalidation and subscription disablement separately.

## 5. Provisional measurement targets

These targets bound the readiness rubric; they are not committed SLOs. The readiness decision recommends production SLOs from observed distributions.

| Measure | Provisional target | Notes |
|---|---|---|
| Warm decision latency p50 | < 15 ms | Strongly-consistent single-key grant lookup + entitlement coverage query. |
| Warm decision latency p95 | < 40 ms | |
| Warm decision latency p99 | < 80 ms | |
| Cold-start-path decision latency p99 | < 300 ms | Includes SDK client init and first cold read. |
| Sustained target throughput | 50 decisions/sec per evaluator instance | Drives the `target-throughput` profile. |
| Revocation: time to first denied decision p95 | < 1 s | From accepted revocation to first denied authorization check. |
| Revocation: complete subscription disablement p95 | < 30 s | From accepted revocation to confirmed disablement of every affected active subscription; bounded by subscription fan-out. |
| Decision error/retry/throttle rate | < 0.5% | Across warm samples. |

## 6. AWS Region and time box

| Parameter | Value |
|---|---|
| AWS Region | `us-east-1` (matches the Cognito issuer in the frozen manifest) |
| Spike time box | 5 working days from sandbox readiness |
| Sandbox isolation | Dedicated AWS account (or isolated OU) with all resources tagged `nexus-spike=evaluate-authorization-engine` |

## 7. Client settings

| Setting | Value | Rationale |
|---|---|---|
| DynamoDB read consistency (decisions) | `StronglyConsistent` | Authorization allow decisions must reflect acknowledged writes. |
| DynamoDB read consistency (GSI admin/candidate lookup) | `EventuallyConsistent` | GSIs serve administration and subscription candidate lookup only, never allow decisions. |
| SDK request timeout | 10 s | Spike tolerance; production tuned separately. |
| Retries on decision reads | 0 | Measure raw latency and failure behavior; no retry masking. |
| Retries on revocation/admin writes | standard SDK backoff | Revocation correctness is mandatory; retries are recorded. |
| Decision cache | none | Every decision reads current grants; required by the revocation contract. |
| Client telemetry | retain raw samples + timestamps | Reproducible environment evidence (task 3.x, 4.x). |

## 8. Reproducible environment capture

Every retained result records: DynamoDB table/GSI names and capacity mode, AWS Region, execution date/time, dataset size (grant count, subscription count), tool versions (spike runner, AWS SDK), workload profile, and the frozen manifest SHA-256. This satisfies the `Reproducible environment` scenario.
