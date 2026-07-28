# Dated Cost Worksheet — DynamoDB grant-engine spike

> Spike artifact (task 4.3). Itemizes low/expected/high monthly cost for the DynamoDB-backed grant
> engine. Prices are a spike estimate, not a quote. **Verify ap-southeast-2 rates in the AWS
> console before any production commitment.**

## 1. Price source and dating

- Source: AWS DynamoDB on-demand pricing page, `https://aws.amazon.com/dynamodb/pricing/on-demand/`
- Page retrieved: 2026-07-28
- Published US East (Ohio) on-demand rates (Standard table class):
  - Write Request Unit (WRU): **$0.625 per million WRUs**
  - Read Request Unit (RRU): **$0.125 per million RRUs**
  - Data storage: **$0.25 per GB-month**
  - PITR backups: **$0.20 per GB-month**
- **ap-southeast-2 (Sydney) estimate**: AWS Sydney on-demand rates run approximately **+20%** over
  US East for DynamoDB throughput and ~+16% for storage. Applied factors below are estimates; the
  exact Sydney rates must be confirmed in the AWS console for the target account.

| Unit | US East (Ohio) — sourced | ap-southeast-2 — estimated | Basis |
|---|---|---|---|
| WRU | $0.625 / 1M | **$0.75 / 1M** | +20% Sydney premium (estimate) |
| RRU | $0.125 / 1M | **$0.15 / 1M** | +20% Sydney premium (estimate) |
| Storage | $0.25 / GB-mo | **$0.29 / GB-mo** | +16% (estimate) |
| PITR | $0.20 / GB-mo | **$0.24 / GB-mo** | +20% (estimate) |

Billing-unit rules (per AWS docs): strongly-consistent read = 1 RRU / 4 KB; eventually-consistent
read = 0.5 RRU / 4 KB; standard write = 1 WRU / 1 KB. GSI writes are charged when the base table
item is written (one WRU per GSI per item write, per 1 KB). GSI reads follow the same RRU rules.

## 2. Volume assumptions (low / expected / high monthly)

| Driver | Low | Expected | High | Notes |
|---|---|---|---|---|
| Authorization decisions | 10 M | 100 M | 1 B | `schema:subscribe`/`discover`/`publish` checks |
| Grant mutations (bindings/entitlements created) | 1 k | 10 k | 100 k | Admin write path |
| Revocation events | 100 | 1 k | 10 k | Each flips a grant + reconciles subscriptions |
| Grant + subscription storage | 1 GB | 10 GB | 100 GB | Grants, bindings, entitlements, subscriptions, audit |
| Affected subscriptions per revocation (fan-out) | 2 | 5 | 20 | Disablement writes |

**Per-decision DynamoDB cost model** (from `src/dynamo.rs` access patterns):

| Decision path | Strongly-consistent reads | Writes |
|---|---|---|
| `schema:publish` | 1 GetItem (1 RRU) | 0 |
| `schema:subscribe` allow/deny | 1 GetItem + 1 Query (2 RRU) | 0 |
| `schema:discover` allow | 1 GetItem + 1 Query + N coverage reads (~2+N RRU) | 0 |
| Weighted average decision | **~2 RRU** | 0 |

## 3. Itemized monthly cost (ap-southeast-2 estimate)

### 3.1 Expected volume (100 M decisions, 10 GB)

| Line item | Units / month | Unit cost | Monthly cost |
|---|---|---|---|
| Decision reads | 200 M RRU (100M × 2) | $0.15 / 1M | $30.00 |
| Grant mutations | 10 k WRU | $0.75 / 1M | $0.01 |
| GSI writes (grant_id-index on mutations) | 10 k WRU | $0.75 / 1M | $0.01 |
| Revocation writes (flip grant + disable fan-out) | 1k × (1 + 5) = 6 k WRU | $0.75 / 1M | $0.00 |
| Revocation candidate reads (GSI + sub scan) | 1k × ~6 RRU = 6 k RRU | $0.15 / 1M | $0.00 |
| Audit — per-decision audit item (OPTION A) | 100 M WRU | $0.75 / 1M | $75.00 |
| Audit — mutations/revocations only (OPTION B) | ~16 k WRU | $0.75 / 1M | $0.01 |
| Data storage | 10 GB | $0.29 / GB | $2.90 |
| PITR (if enabled) | 10 GB | $0.24 / GB | $2.40 |
| CloudWatch alarms + metrics | ~10 alarms | ~$0.30/alarm | $5.00 |
| KMS / CloudTrail / data transfer | — | — | ~$2.00 |
| **Total — Option A (per-decision audit)** | | | **~$117.33** |
| **Total — Option B (audit on mutations only)** | | | **~$42.34** |

### 3.2 Low volume (10 M decisions, 1 GB)

| Line item | Monthly cost |
|---|---|
| Decision reads (20 M RRU) | $3.00 |
| Audit Option A (10 M WRU) | $7.50 |
| Audit Option B | $0.01 |
| Storage (1 GB) + PITR | $0.53 |
| Monitoring + misc | $7.00 |
| **Total — Option A / Option B** | **~$18.03 / ~$10.54** |

### 3.3 High volume (1 B decisions, 100 GB)

| Line item | Monthly cost |
|---|---|
| Decision reads (2 B RRU) | $300.00 |
| Audit Option A (1 B WRU) | $750.00 |
| Audit Option B | $0.08 |
| Storage (100 GB) + PITR | $53.00 |
| Monitoring + misc | $12.00 |
| **Total — Option A / Option B** | **~$1115.08 / ~$365.16** |

## 4. Findings and recommendations

- **Decision reads dominate variable cost** and scale linearly with decision volume. At the expected
  100 M decisions/month, strongly-consistent decision reads cost ~$30/month in ap-southeast-2 —
  inexpensive and squarely within a "cheap enough" range for an authorization engine.
- **Audit strategy is the dominant cost lever.** Writing one audit item per decision (Option A) more
  than doubles total cost and becomes the largest line item at scale (1 B decisions → $750/month).
  Auditing only mutations and revocations (Option B) keeps cost trivial. **Recommendation**: do not
  write a per-decision DynamoDB audit item; rely on CloudTrail (API-level) + an audit item on
  mutations/revocations only, or sample per-decision audit at a configurable rate. This is a
  production design decision to confirm in the implementation change.
- **Revocation cost is negligible** at all modeled volumes (fan-out is small; revocations are rare
  relative to decisions). Synchronous subscription reconciliation is cheap.
- **Provisioned vs on-demand**: at expected volume, 100 M reads/month ≈ 2 M RRUs/day average ≈ ~23
  RRU/s average, but peak/throughput tests sustained 137-508 dec/s (274-1016 RRU/s peak). On-demand
  handles peaks without provisioning; provisioned capacity with auto-scaling + Database Savings
  Plans (up to 18% on-demand savings) would lower steady-state cost once the workload is predictable.
  For the spike and early production, on-demand is the right choice.
- **GSI cost** (grant_id-index + administration GSIs) is driven by base-table writes, which are rare
  (mutations/revocations), so GSI cost is negligible. GSI storage adds to the storage line.
- **Storage is trivial** at all modeled volumes (<$53/month even at 100 GB).

## 5. Caveats

- All ap-southeast-2 unit prices are **estimates** (US East sourced + regional premium). Confirm
  exact rates in the AWS console for the production account before quoting budgets.
- The model assumes the evaluator runs **in-region** (no cross-Region data transfer). Cross-Region
  reads would add data-transfer-out cost and are not recommended for the decision path.
- PITR is optional; included only to show its marginal cost. Backup/restore and CDC (Streams/Kinesis)
  are not required by the grant engine and are excluded.
- The worksheet does not include the hosting cost of the evaluator compute (Lambda/EC2); that is
  priced under the Lambda/compute line of the platform, not the grant engine.
