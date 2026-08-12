# In-Region Decision Latency — add-grant-engine (tasks 6.1/6.2)

> Measured 2026-08-05 with `make benchmark-grants` (Lambda `konductor-grant-benchmark`,
> ap-southeast-2) against the deployed `konductor-authorization-bindings` table. The harness
> reuses the spike's declared measurement procedure
> (`evaluate-authorization-engine/evidence/methodology.md`): sequential warm samples, SDK
> retries disabled (max attempts 1), no decision cache, and the real `DynamoGrantEngine`
> decision path. This closes the spike's primary residual risk: latency targets were
> conditional on an in-region measurement.

## 1. Execution environment

- Evaluator: Lambda `konductor-grant-benchmark` (provided.al2023, arm64, 256 MB) in
  ap-southeast-2, account 444433332222 — same region and account as the table
- Table: `konductor-authorization-bindings` (PAY_PER_REQUEST, KMS, PITR)
- Decision path measured: `schema:publish` = one strongly consistent GetItem per candidate
  principal (one candidate per probe here)
- Samples: 200 per probe after 20 warmup samples; 4 probes = 800 measured decisions
- Fixtures: two harness-seeded bindings on `benchmark-domain`
  (cognito-group `benchmark-publishers`, aws-account `000000000001`), removed after the run
- Cold start: **Init Duration 111.62 ms** (recommended SLO ≤ 500 ms — pass);
  full first invocation 2385 ms including seeding, 880 decisions, and cleanup

## 2. Results vs. recommended SLOs (`schema:publish`-class: p50 ≤ 5 ms, p99 ≤ 20 ms)

| Probe | Expected | n | p50 (ms) | p95 (ms) | p99 (ms) | max (ms) | Wrong decisions |
|---|---|---|---|---|---|---|---|
| cognito-publish-allow | allow | 200 | 1.56 | 9.87 | 11.66 | 12.48 | 0 |
| cognito-publish-deny | deny | 200 | 1.92 | 6.95 | 8.91 | 9.91 | 0 |
| aws-publish-allow | allow | 200 | 1.46 | 10.06 | 11.73 | 12.33 | 0 |
| aws-publish-deny | deny | 200 | 2.18 | 5.32 | 8.11 | 8.68 | 0 |

**Result: all probes pass with wide margin.** Worst p50 = 2.18 ms (target ≤ 5 ms);
worst p99 = 11.73 ms (target ≤ 20 ms). 0 wrong decisions across 800 samples.
No remediation-ladder step applies.

## 3. Interpretation

- Confirms the spike's in-region projection for the publish path (~3–5 ms p50); observed
  p50 is 1.5–2.2 ms, consistent with a single in-region strongly consistent GetItem.
- Allow/deny p50 differences (1.5 vs 2.2 ms) are noise-level; both paths perform exactly
  one GetItem.
- The fail-closed error path is not separately measurable here (no failures occurred);
  it is covered by the `dynamo_store_error_fails_closed` parity test.
- `schema:subscribe`- and `schema:discover`-class SLOs (2 and 2+N reads) are measured when
  `add-subscriber-entitlements` lands those paths; the publish-class result bounds their
  per-read cost at ~2 ms.

## 4. Reproduction

```bash
make benchmark-grants     # deploys konductor-grant-benchmark, invokes, prints the report
make teardown-benchmark   # deletes the throwaway stack
```

The stack is inert between runs (no triggers); fixtures are removed by every run.
