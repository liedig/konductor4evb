# Production SLO Recommendations — DynamoDB grant-engine spike

> Spike artifact (task 5.4). Evidence-based recommendations derived from `bench-latency.md` and
> `cost-worksheet.md`. These are draft production SLOs for the implementation change to finalize,
> **not** commitments. All latency recommendations assume an **in-region evaluator** co-located with
> the DynamoDB grant table; the spike's remote-client measurements establish access-pattern
> correctness and error behavior but are RTT-dominated (see `bench-latency.md` Section 7).

## 1. Decision latency SLOs (in-region)

The spike measured per-decision DynamoDB call counts: `publish` = 1 strongly-consistent read,
`subscribe` = 2, `discover` = 2 + N (schema fan-out). With in-region DynamoDB RTT (~2-4 ms), the
projected p50 is ~3-5 ms (publish), ~5-9 ms (subscribe), ~8-15 ms (discover, small N). Recommended
production SLOs (per evaluator instance, in-region, steady-state warm):

| SLO | Recommended target | Spike basis |
|---|---|---|
| `schema:publish` p50 / p99 | ≤ 5 ms / ≤ 20 ms | 1 strongly-consistent GetItem; in-region projection |
| `schema:subscribe` p50 / p99 | ≤ 10 ms / ≤ 40 ms | 2 strongly-consistent reads; in-region projection |
| `schema:discover` p50 / p99 | ≤ 15 ms / ≤ 60 ms | 2 + N reads; bound N via pagination/cap |
| Cold-start (Lambda init) p99 | ≤ 500 ms | SDK client init + first read; confirm in-region |
| Decision error rate | ≤ 0.1% | Spike observed 0.000% across 2040 samples |

The spike's provisional p50 < 15 ms / p95 < 40 ms / p99 < 80 ms targets are **achievable in-region**
for publish and subscribe; discover at the bound depends on schema fan-out (cap N, or serve the
visible-schema list from a cached/materialized projection if fan-out is large).

## 2. Throughput SLO

- **Recommended**: ≥ 200 decisions/sec sustained per in-region evaluator instance at p99 ≤ 40 ms.
- Spike basis: a single **remote** client sustained 137-508 decisions/sec (50 in-flight) with 0
  errors; an in-region evaluator with ~10x lower per-call RTT sustains substantially more. 50
  dec/sec (the provisional target) is comfortably exceeded.
- Horizontal scaling: the engine is stateless per-instance (no decision cache; strongly-consistent
  reads), so capacity scales linearly with evaluator instances. No shared hot state.

## 3. Revocation SLOs

The spike adapter reconciles subscriptions **synchronously** within the `revoke()` call, so
disablement completes when revocation is acknowledged. Recommended SLOs:

| SLO | Recommended target | Spike basis |
|---|---|---|
| Time to first denied decision p95 | ≤ 250 ms | spike 240 ms (remote); in-region ≈ tens of ms |
| Complete subscription disablement p95 | ≤ 1 s | spike 721 ms (remote, synchronous reconcile); in-region ≈ tens of ms |
| Revocation correctness | 100% | 30/30 trials: sole grant revoked → subscription disabled + next decision denied; overlapping preserved |

The provisional < 1 s / < 30 s targets are met even from a remote client. For production, tighten
to ≤ 250 ms / ≤ 1 s (in-region). If eventual consistency is ever introduced for the subscription
reverse-index, the disablement SLO must be re-bounded by GSI lag (the spike avoids this by
reconciling from a strongly-consistent scan at revocation time).

## 4. Asynchronous-handling SLOs

- Deterministic denials (replay, incomplete context, tampered context, revoked-before-consume) MUST
  be acknowledged exactly once, never retried on the decision path. Spike confirms all variants
  return the correct denial with SDK retries disabled.
- Recommended: the async command consumer uses a per-command idempotency key (`command_id`) with the
  in-memory `seen_commands` set backed by a durable store in production (the spike's in-process set
  is not durable across restarts). SLO: 0 duplicate application of a command with the same
  `command_id`.

## 5. Cost SLO / budget guidance

- Expected volume (100 M decisions/month, in-region, Option B audit): **~$42/month** in
  ap-southeast-2. See `cost-worksheet.md`.
- The dominant cost lever is the **audit strategy**: avoid per-decision DynamoDB audit items;
  audit mutations/revocations only (+ CloudTrail). This keeps variable cost driven by decision reads
  (~$30/month at expected volume).
- Recommended budget guardrail: alert if monthly RRU spend exceeds 2x the expected decision-read
  cost (signals a hot path or missing binding/entitlement causing repeated denials).

## 6. Conditions and residual risks carried into the readiness decision

1. **Latency targets are conditional on in-region deployment.** The spike did not measure from an
   in-region host; an in-region re-measurement (EC2/Lambda in the target Region) is required before
   the latency SLOs are considered validated. This is the primary residual risk.
2. **Discover fan-out**: `schema:discover` latency scales with the number of schemas on a producer
   domain. If producer domains grow large, cap N or materialize the visible-schema list.
3. **Audit durability**: the spike's `seen_commands` set is in-process; production needs a durable
   idempotency store for the async contract.
4. **Concurrency on grant mutation**: the spike serializes via in-process state / DynamoDB
   conditional writes are not yet exercised; production grant mutation needs optimistic concurrency
   or transactions (operational exercise 6.2).
