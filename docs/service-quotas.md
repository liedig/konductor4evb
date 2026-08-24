# Service Quotas and Capacity Planning

Konductor for Amazon EventBridge provisions real AWS resources for every domain and every subscription. Several of the services it builds on have **default quotas low enough to block provisioning after only a handful of domains** — the first one you will hit is the EventBridge Schema Registry limit of **10 registries per region**, which fails the fourth domain:

```json
{
  "resourceType": "aws-sdk:schemas",
  "resource": "createRegistry",
  "error": "Schemas.TooManyRequestsException",
  "cause": "The limit has been reached for total number of Registries. The current limit is 10."
}
```

This guide tells you which quotas to raise in the **platform account** before onboarding domains at scale, with concrete targets for 20, 50, and 100 domains.

> **TL;DR** — before onboarding more than 3 domains, raise **Schema Registries** (`schemas` / `L-85663EFB`); before ~30 domains, raise **Event buses** (`events` / `L-658A4FD9`). Everything else has generous defaults or scales with subscriptions/traffic rather than domain count.

All quota values below are AWS defaults as of July 2026, per account per region. Verify current values with `aws service-quotas list-aws-default-service-quotas --service-code <code>`.

---

## The 1,000-domain ceiling

The platform enforces a hard ceiling of **1,000 rows** in the domains table (`konductor-domains`), enforced atomically at registration: every row-creating write (a new `PENDING_APPROVAL` registration, or a collision-preflight `REJECTED` row) increments a counter item (`domainName = "META#domain-count"`, attribute `count`) in the same DynamoDB transaction, conditioned below the cap. Re-registration of an existing rejected domain overwrites its row without charging the counter. At the cap, registration fails deterministically with `RegisterDomainFailed` (reason: domain limit exceeded) — no approval workflow starts.

Two properties shape operations:

- **The counter never decrements.** There is no domain-deletion capability yet, so the ceiling bounds the table's *lifetime* registrations, including rejected rows. Retiring stale or rejected domains before a future pruning capability lands requires deleting their rows and decrementing the counter by the same amount (see reconciliation below).
- **Capacity alarm.** The hourly `konductor-domain-count-reporter` function publishes the counter as the `DomainCount` metric (namespace `Konductor/DomainManagement`); `konductor-domain-count-capacity-alarm` fires at **800** (80%). When it fires, reconcile the counter, clean up stale rows, and confirm quota headroom before the next onboarding wave.

Quota consequence: the ceiling caps N at 1,000, so with E environments the most you will ever need is **1,000 × E registries** (`schemas` / `L-85663EFB`) and **1,000 × E + 2 event buses** (`events` / `L-658A4FD9`) — 3,000 and 3,002 at the default E = 3. Request only for the N you actually plan to onboard (checklist below); the ceiling guarantees quota requests above 1,000 × E are never needed.

### Seeding the counter on existing deployments

Fresh deployments self-seed: the first registration creates the counter at zero and increments it in one transaction. Deployments that already had domains **before** the ceiling shipped must seed the counter once, immediately after upgrading the stack, or the pre-existing rows consume headroom the counter cannot see:

```bash
export DOMAINS_TABLE=konductor-domains   # adjust if you renamed the table

# 1. Count existing domain rows (paginated; excludes the counter item itself)
TOTAL=0; TOKEN=""
while :; do
  ARGS=(--table-name "$DOMAINS_TABLE" --select COUNT
        --filter-expression "domainName <> :counter"
        --expression-attribute-values '{":counter":{"S":"META#domain-count"}}')
  [ -n "$TOKEN" ] && ARGS+=(--starting-token "$TOKEN")
  PAGE=$(aws dynamodb scan "${ARGS[@]}" --output json)
  TOTAL=$((TOTAL + $(jq -r '.Count' <<<"$PAGE")))
  TOKEN=$(jq -r '.NextToken // empty' <<<"$PAGE")
  [ -z "$TOKEN" ] && break
done
echo "rows: $TOTAL"

# 2. Seed the counter — fails if it already exists, so re-running is safe
aws dynamodb put-item --table-name "$DOMAINS_TABLE" \
  --item '{"domainName":{"S":"META#domain-count"},"count":{"N":"'"$TOTAL"'"}}' \
  --condition-expression "attribute_not_exists(domainName)"
```

### Reconciliation: counter versus row count

Because rows are never deleted, the counter must always equal the row count. Investigate drift (rows created out-of-band, manual edits, a partially seeded counter) with:

```bash
# Row count: the paginated scan from step 1 above
# Counter value:
aws dynamodb get-item --table-name "$DOMAINS_TABLE" \
  --key '{"domainName":{"S":"META#domain-count"}}' \
  --consistent-read --query 'Item.count.N'
```

If they differ, repair during a quiet window (an in-flight registration increments the counter between your scan and your write, so pause onboarding first), setting the counter to the true row count:

```bash
aws dynamodb update-item --table-name "$DOMAINS_TABLE" \
  --key '{"domainName":{"S":"META#domain-count"}}' \
  --update-expression "SET #c = :v" \
  --expression-attribute-names '{"#c":"count"}' \
  --expression-attribute-values '{":v":{"N":"'"$TOTAL"'"}}'
```

---

## Resource consumption model

Domains are provisioned once per environment in the platform-configured
`Environments` list (default `dev,staging,prod`, so **E = 3** below). **N** is
the number of ACTIVE domains, **S** the number of active subscriptions.

Platform account:

| Resource | Count | Created by |
|---|---|---|
| Domain event bus | N × E | `provision-env` |
| Schema registry | N × E | `provision-env` |
| Subscription DLQ (SQS) | N × E | `provision-env` |
| CloudWatch dashboard | N × E | `provision-env` |
| CloudWatch alarms (failed-invocations, DLQ-depth) | 2 × N × E | `provision-env` |
| SSM Advanced Parameter (`/namespace/{domain}`) | N | `provision-domain` |
| SSM parameter in RAM resource share | N | `provision-domain` |
| EventBridge rule on producer's domain bus | S × E | `activate-subscription` |
| Zero-events alarm | S × E | `activate-subscription` |
| Singletons (konductor-bus, 2 DynamoDB tables, API, 13 Lambdas, 7 state machines, 6 queues) | fixed | core stack |

Each domain account: E app event buses, E integration DLQs, 1 trust role. None of these approach any default quota while an account hosts a single domain (the intended model).

---

## Quotas that break first (domain count)

With E = 3, required capacity at each sampling point (add ~20% headroom on top; smoke tests and failed-provisioning leftovers consume real quota until cleaned up):

| Quota | Service code / quota code | Default | Formula | 20 domains | 50 domains | 100 domains | Breaks at |
|---|---|---|---|---|---|---|---|
| **Registries** | `schemas` / `L-85663EFB` | **10** | N × E | 60 → request **75** | 150 → request **180** | 300 → request **360** | **N = 3** |
| **Event buses** | `events` / `L-658A4FD9` | 100 | N × E + 2 | 62 → OK | 152 → request **200** | 302 → request **360** | N = 32 |
| Resources per RAM share | `ram` | 500 | N | OK | OK | OK | N = 500 |
| SSM advanced parameters | `ssm` | 100,000 | N | OK | OK | OK | — |

Everything else that scales with N (SQS queues, CloudWatch dashboards and alarms) has **no count quota** — those are cost items, covered below.

> The Registries default of 10 is the reason a fresh platform account can fully provision only **three** domains. Request the increase before your first production onboarding; Service Quotas requests for this quota are typically auto-approved at moderate values but can take hours to days.

## Quotas that break with subscriptions and schemas

These scale with usage patterns rather than domain count:

| Quota | Service / code | Default | What consumes it | Practical ceiling |
|---|---|---|---|---|
| **Rules per event bus** | `events` / `L-244521F2` | 300 | One rule per subscription per env, on the **producer's** bus | 300 active subscriptions targeting any single producer domain (per env). Raise it for hub domains that many consumers subscribe to. |
| Schemas per registry | `schemas` / `L-EE9E5FA9` | 100 | One schema per event type per env | 100 distinct event types per domain |
| Schema versions per schema | `schemas` / `L-3C443A2A` | 100 | Each published revision | 100 revisions of one event type |
| Event bus policy size | `events` / `L-FC354966` | 10,240 chars | **Does not grow** — one fixed statement per bus (`allow-platform-subscription-delivery` on app buses; one producer grant on platform buses) | Not a concern. This is a deliberate design property of resource-policy delivery: per-subscription IAM grants would have grown this policy linearly. |
| Targets per rule | `events` (fixed) | 5 | Konductor attaches exactly 1 target per subscription rule | Not a concern |

## Data-plane throughput (traffic, not counts)

Both quotas below are **regional and per account**, so the platform account is the aggregation point for the entire organization's event traffic. Defaults for `ap-southeast-2` (they vary by region — check yours):

| Quota | Code | Default (ap-southeast-2) | What consumes it |
|---|---|---|---|
| PutEvents TPS | `events` / `L-9B653E91` | 1,200/s | Cross-bus **delivery**: every event forwarded to a consumer app bus is a PutEvents call made under the platform delivery role, counted against the **platform** account. (Producers publishing into their platform bus consume their **own** account's PutEvents quota.) |
| Invocations TPS | `events` / `L-5540C5E3` | 2,250/s | Every rule match on a platform bus. An event matched by M subscriptions = M invocations. |

Rule of thumb: total delivered events/sec across **all** domains and environments must fit inside the platform account's PutEvents quota. At 100 domains, if each averages just 15 delivered events/sec, you're at the default ceiling. Both quotas are adjustable — size them from your aggregate expected event rate with peak headroom, and revisit whenever a high-volume domain onboards.

## Control-plane throttling during provisioning

These are TPS limits, not counts. `provision-domain` fans out over environments with unbounded Map concurrency, so parallel provisioning bursts against them. Every workflow SDK step retries throttling errors with exponential backoff and jitter (throttle-prone CloudWatch/Schemas calls get 5 attempts; standard control-plane calls get 3), so transient throttles are absorbed rather than failing the environment.

| API | Default TPS | Adjustable | Exposure |
|---|---|---|---|
| EventBridge control plane (CreateEventBus, PutPermission, PutRule, …) | 50/s shared | No | Safe for interactive onboarding; batch operations (backfill scripts, mass re-subscription) should pace themselves |
| CloudWatch PutMetricAlarm | **3/s** | Yes (`monitoring` / `L-0720E68F`) | Each domain provisioning fires 2 alarms × E envs in parallel (6 calls). Workflow retries absorb collisions from a few concurrent approvals; raise the quota for sustained batch onboarding. |
| CloudWatch PutDashboard | 10/s | Yes | E parallel calls per provisioning; low risk |
| Schemas CreateRegistry | throttled (429 shares the `TooManyRequestsException` error code with the count limit — read the message) | — | E parallel calls per provisioning |

If a provisioning still fails after retry exhaustion (sustained throttling, e.g. a large batch onboarding), re-approve the domain — provisioning is idempotent after rollback — and consider raising `L-0720E68F` to 10/s for the onboarding window. Note the schemas caveat: a `Schemas.TooManyRequestsException` caused by the **registry count limit** (not rate) will be retried for ~1 minute before failing; the error message distinguishes the two.

## Other platform-account limits (fine by default, listed for completeness)

| Service | Quota | Default | Konductor usage |
|---|---|---|---|
| Lambda | Account concurrent executions | 1,000 (new accounts sometimes start lower — verify) | 13 functions share the pool; SQS-triggered handlers scale with command volume |
| Step Functions | Open standard executions | 1,000,000 | One execution per registration/approval/provisioning/subscription operation |
| API Gateway | Account throttle | 10,000 RPS | Template self-throttles the stage to 50 rps / 100 burst |
| DynamoDB | Tables | 2,500 | 2 tables, on-demand |
| IAM | Roles | 1,000 | ~8 platform roles, fixed |
| CloudFormation | Resources per stack | 500 | Core stack ≈ 70 |
| SES (if notifications enabled) | Sandbox: 200 emails/day, 1/s, verified recipients only | — | ~2–3 emails per registration. Request SES production access before real onboarding, or operators must pre-verify every owner address. |

Domain accounts need **no quota increases** under the one-domain-per-account model.

## Cost scaling (no quota, but grows with N)

Counts have no ceiling but the bill grows linearly; at 100 domains these dominate the platform account's fixed costs (us-east-1-equivalent pricing):

| Item | Unit cost | 20 domains | 50 domains | 100 domains |
|---|---|---|---|---|
| Dashboards (N × E, first 3 free) | $3/mo | ~$171/mo | ~$441/mo | ~$891/mo |
| Alarms (2 × N × E + S × E) | $0.10/mo | $12+/mo | $30+/mo | $60+/mo |
| SSM advanced parameters (N) | $0.05/mo | $1/mo | $2.50/mo | $5/mo |

The per-env dashboard is the standout: at scale, consider whether every domain × env needs a standing dashboard or whether a consolidated/on-demand view should replace it (a future design decision — see `docs/development/decisions.md`).

## Requesting increases

```bash
# Check a current applied quota (example: schema registries)
aws service-quotas get-service-quota \
  --service-code schemas --quota-code L-85663EFB

# Request an increase (example: 100 domains × 3 envs + headroom)
aws service-quotas request-service-quota-increase \
  --service-code schemas --quota-code L-85663EFB --desired-value 360

aws service-quotas request-service-quota-increase \
  --service-code events --quota-code L-658A4FD9 --desired-value 360

# Track open requests
aws service-quotas list-requested-service-quota-change-history \
  --service-code schemas
```

Increases are per region — request them in the region where `konductor-core` is deployed. Large jumps may route to AWS Support for review; request them ahead of onboarding waves, not during.

## Pre-onboarding checklist

Before an onboarding wave that takes you to **N** total domains (E = envs in the `Environments` parameter):

- [ ] N ≤ 1,000 (the platform ceiling) with headroom — check the `DomainCount` metric or `konductor-domain-count-capacity-alarm` state
- [ ] Schema Registries quota ≥ N × E × 1.2 (`schemas` / `L-85663EFB`)
- [ ] Event buses quota ≥ (N × E + 2) × 1.2 (`events` / `L-658A4FD9`)
- [ ] Rules-per-bus quota reviewed for any producer expecting > ~250 subscriptions (`events` / `L-244521F2`)
- [ ] PutEvents / Invocations TPS sized from aggregate expected event rate (`L-9B653E91`, `L-5540C5E3`)
- [ ] PutMetricAlarm rate raised for sustained batch onboarding (`monitoring` / `L-0720E68F`) — occasional concurrent approvals are absorbed by workflow retries
- [ ] SES production access granted (if notifications enabled)
- [ ] Stale smoke-test registries/buses cleaned up (they consume the same quotas)
