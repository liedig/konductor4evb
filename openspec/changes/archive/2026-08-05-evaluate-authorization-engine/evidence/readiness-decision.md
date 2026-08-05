# Readiness Decision — DynamoDB grant-engine spike

> Spike artifact (tasks 7.1, 7.2). Applies the mandatory correctness/non-disclosure behavior and the
> frozen readiness rubric (`readiness-rubric.md`) to the collected evidence. The two human-review
> gates (6.4, 7.3) were closed by operator direction on 2026-07-28. The decision is **conditionally
> ready**: the single residual is in-region latency validation, accepted as a documented residual
> uncertainty (operator decision; in-region re-measurement not pursued this spike).

## 1. Decision

**DynamoDB-backed grants are conditionally ready as Nexus's initial authorization engine.**

The design satisfies mandatory correctness and non-disclosure behavior in-cloud, is cheap, scales,
and is operationally simple. The single condition is **in-region latency validation**: the spike
measured from a remote client (RTT-dominated), so the provisional latency targets are met only by
in-region projection, not by direct measurement. Confirming the targets requires re-running the
benchmark from an in-region host (EC2/Lambda in the production Region). The two human-review gates
(6.4, 7.3) were closed by operator direction on 2026-07-28; the in-region re-measurement was not
pursued this spike and is carried as residual risk #1 into the implementation change. The decision
is therefore conditionally ready, not draft.

## 2. Evidence against the readiness rubric

| Rubric criterion | Evidence | Verdict |
|---|---|---|
| Mandatory correctness (all frozen scenarios) | `local-correctness.md`, `cloud-correctness.md`: 13/13 scenarios, 34/34 steps, in-memory and DynamoDB | support |
| Mandatory non-disclosure | S12a/S08c `deny-as-unknown` indistinguishable from unknown; S12c list filtering | support |
| Decision latency | `bench-latency.md`: remote p50 146 ms (RTT-dominated); in-region projection 3-15 ms | conditional (in-region re-measure) |
| Throughput | 137-508 dec/s sustained (remote, 50 in-flight); exceeds 50 dec/s target | support |
| Error/throttle/retry rate | 0.000% across 2040 samples | support |
| Cold-start | remote p99 ~460-560 ms (RTT-dominated); in-region Lambda warm start expected < 300 ms | conditional (in-region re-measure) |
| Cost | `cost-worksheet.md`: ~$42/month expected (Option B audit) | support |
| Revocation correctness + timing | 30/30 trials; first-denied p95 240 ms, disablement p95 721 ms | support |
| Asynchronous handling | all variants (enqueue, delayed, revocation-before-consume, replay, incomplete, tampered) | support |
| Operational simplicity | `operational-exercises.md`: low complexity, standard controls to add | support |
| Local-testability | in-memory parity, shared runner, frozen fixtures | support |

**Production-blocker criteria** (from `readiness-rubric.md`): none triggered. No mandatory behavior
failed; no unsupported scenario in the common suite; no untraceable conclusion.

## 3. Recommended production SLOs (summary)

From `slo-recommendations.md` (in-region, per evaluator instance):

- `schema:publish` p50 ≤ 5 ms / p99 ≤ 20 ms
- `schema:subscribe` p50 ≤ 10 ms / p99 ≤ 40 ms
- `schema:discover` p50 ≤ 15 ms / p99 ≤ 60 ms (cap schema fan-out)
- Sustained throughput ≥ 200 dec/s per instance; horizontal scaling linear
- Revocation: first denied p95 ≤ 250 ms; complete disablement p95 ≤ 1 s
- Decision error rate ≤ 0.1%
- Cost guardrail: alert if monthly RRU spend > 2x expected decision-read cost

## 4. AVP as the rejected alternative

Amazon Verified Permissions was evaluated and **rejected as the initial engine** (captured in
`design.md` and the explore-phase decision). Rationale:

- Nexus authorization is domain-pairing grants (consumer/producer/schema coverage), not a rich
  Cedar policy hierarchy; a dedicated policy engine adds learning/cost/latency surface without
  matching expressive need at this stage.
- DynamoDB-backed grants reuse the team's existing DynamoDB skills and infrastructure, keep the
  decision path to 1-2 strongly-consistent reads, and are trivially cheap.
- The grant-table model is auditable (every allow traces to an explicit grant item) and locally
  testable without AWS.

**Reevaluation triggers** (also in `design.md`): revisit AVP if (a) a requirement emerges for
fine-grained attribute-based or hierarchical policies that explicit grants cannot express cleanly;
(b) cross-account principal resolution needs a federated policy plane beyond Cognito groups; (c)
policy-authoring volume makes hand-maintained grant items uneconomical; (d) a compliance regime
mandates an externalized, auditable policy store.

## 5. Assumptions, trade-offs, residual risks

- **Assumption**: the evaluator runs in-region with the grant table. The spike did not measure this.
- **Trade-off**: explicit grants (no implicit inheritance) maximize predictability/auditability at
  the cost of expressive convenience for any future hierarchical need (mitigated by reevaluation
  triggers).
- **Residual risks** (carried into the implementation change):
  1. In-region latency unmeasured — primary condition on this decision.
  2. Concurrent grant mutation needs conditional writes/transactions (not exercised in spike).
  3. Durable idempotency store needed for the async `command_id` contract (spike uses in-process set).
  4. `schema:discover` fan-out must be capped or materialized for large producer domains.
  5. Semantic audit items + time-ordered admin GSI to be provisioned in production.

## 6. Conditional remediation

If in-region latency measurement does **not** meet the recommended SLOs, remediation options
(shortest first): (1) co-locate evaluator and table in the same Region/AZ; (2) reduce per-decision
read count (e.g. denormalize binding+entitlement into one item for the hot subscribe path); (3)
introduce a short-TTL safe-cache for the binding lookup only (never the entitlement/coverage check,
to preserve the revocation contract); (4) reevaluate AVP. None of these require changing the
authorization contract or the frozen scenarios.

## 7. Review gates and accepted residual

- [x] **6.4** Two-reviewer qualitative verification — closed by operator direction (2026-07-28).
- [x] **7.3** Independent review — closed by operator direction (2026-07-28).
- [x] **7.4** Destroy tagged sandbox resources (completed 2026-07-28: `nexus-spike-auth-grant`
      and `nexus-spike-auth-sub` deleted from `ap-southeast-2`, account `444433332222`;
      `describe-table` returns `ResourceNotFoundException` for both; no `nexus-spike` tables
      remain). Sanitized evidence retained under `evidence/`.
- **Accepted residual uncertainty**: in-region latency re-measurement (EC2/Lambda in the production
  Region) was not pursued this spike (operator decision, 2026-07-28). The latency verdict remains
  conditional on the in-region projection and is carried as residual risk #1 into the implementation
  change, where an in-region measurement should confirm the recommended SLOs.
