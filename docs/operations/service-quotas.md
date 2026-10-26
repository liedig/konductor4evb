# Service quotas and capacity planning

Konductor for Amazon EventBridge creates AWS resources for each domain and subscription. Check service quotas before onboarding domains to avoid provisioning failures.

With the default three environments, the first constraint is the Amazon EventBridge quota of **10 schema registries per AWS Region**. Provisioning a fourth domain exceeds this quota:

```json
{
  "resourceType": "aws-sdk:schemas",
  "resource": "createRegistry",
  "error": "Schemas.TooManyRequestsException",
  "cause": "The limit has been reached for total number of Registries. The current limit is 10."
}
```

This guide tells you which quotas to raise in the **platform account** before onboarding domains at scale, with concrete targets for 20, 50, and 100 domains.

> **Before onboarding:** Raise **Schema registries** (`schemas`, `L-85663EFB`) before exceeding three domains. Raise **Event buses** (`events`, `L-658A4FD9`) before reaching about 30 domains. Review the remaining quotas against subscription counts and traffic.

The quota values in this guide are AWS defaults as of July 2026, per account per Region. To verify current defaults, use `aws service-quotas list-aws-default-service-quotas --service-code <code>`.

---

## The 1,000-domain ceiling

The platform limits the domains table (`konductor-domains`) to **1,000 domain rows**. Each registration that creates a row increments a counter in the same Amazon DynamoDB transaction. This includes new `PENDING_APPROVAL` registrations and collision-preflight `REJECTED` rows.

The counter item uses `domainName = "META#domain-count"` and the `count` attribute. The transaction succeeds only below the cap. Re-registering an existing rejected domain replaces its row without incrementing the counter. At the cap, registration emits `RegisterDomainFailed` with the reason `domain limit exceeded`. No approval workflow starts.

Two properties shape operations:

- **The counter never decrements.** Neither rejection nor [domain teardown](domain-recovery.md#cleaning-up-domain-resources) lowers it, so the ceiling bounds the table's lifetime registrations, including rejected and torn-down rows. After rows are deleted, by the teardown script or by hand, reconcile the counter to recover their headroom. See [Reconciliation](#reconciliation-counter-versus-row-count).
- **Capacity alarm.** The hourly `konductor-domain-count-reporter` function publishes the counter as the `DomainCount` metric (namespace `Konductor/DomainManagement`); `konductor-domain-count-capacity-alarm` fires at **800** (80%). When it fires, reconcile the counter, clean up stale rows, and confirm quota headroom before the next onboarding wave.

With a ceiling of 1,000 domains and E environments, the model requires at most **1,000 × E registries** and **1,000 × E + 2 event buses**. At the default E = 3, these totals are 3,000 and 3,002. Size requests for the domains you plan to onboard. See the [Pre-onboarding checklist](#pre-onboarding-checklist).

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

Without deletions, the counter equals the row count. Each torn-down or manually deleted row leaves the counter one higher than the row count, and rows created out-of-band, manual edits, or a partially seeded counter can skew it either way. Compare the two with:

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
`Environments` list (default `dev,staging,prod`, so **E = 3** in this model). **N** is
the number of ACTIVE domains, **S** the number of active subscriptions.

The platform account contains the following resources. A dead-letter queue (DLQ) retains messages after delivery retries fail. Amazon Simple Queue Service (Amazon SQS) provides these queues. AWS Systems Manager (SSM) parameters store namespaces, and AWS Resource Access Manager (AWS RAM) shares them.

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

Each domain account hosts one trust role, plus the app event bus and integration DLQ for every environment that resolves into it. By default, all E environments of its domain resolve there. If a registration pins some of them to other accounts with `environmentAccounts`, only the rest resolve there. Per-account headroom therefore counts **environments resolving into the account**, not domains: an account hosting several domains' `prod` environments carries one bus and one DLQ per hosted environment. None of these approach any default quota while an account hosts a single domain's environments (the intended model).

---

## Quotas that break first (domain count)

With E = 3, required capacity at each sampling point (add ~20% headroom on top; smoke tests and failed-provisioning leftovers consume real quota until cleaned up):

| Quota | Service and quota codes | Default | Formula | 20 domains | 50 domains | 100 domains | Breaks at |
|---|---|---|---|---|---|---|---|
| **Registries** | `schemas` / `L-85663EFB` | **10** | N × E | 60 → request **75** | 150 → request **180** | 300 → request **360** | **N = 3** |
| **Event buses** | `events` / `L-658A4FD9` | 100 | N × E + 2 | 62 → OK | 152 → request **200** | 302 → request **360** | N = 32 |
| Resources per RAM share | `ram` | 500 | N | OK | OK | OK | N = 500 |
| SSM advanced parameters | `ssm` | 100,000 | N | OK | OK | OK | Not applicable |

Other resources that scale with N (SQS queues, Amazon CloudWatch dashboards, and alarms) have no count quota in this model. See [Cost scaling](#cost-scaling-no-quota-but-grows-with-n).

> The Registries default of 10 is the reason a fresh platform account can fully provision only **three** domains. Request the increase before your first production onboarding; Service Quotas requests for this quota are typically auto-approved at moderate values but can take hours to days.

## Quotas that break with subscriptions and schemas

These scale with usage patterns rather than domain count:

| Quota | Service / code | Default | What consumes it | Practical ceiling |
|---|---|---|---|---|
| **Rules per event bus** | `events` / `L-244521F2` | 300 | One rule per subscription per env, on the **producer's** bus | 300 active subscriptions targeting any single producer domain (per env). Raise it for hub domains that many consumers subscribe to. |
| Schemas per registry | `schemas` / `L-EE9E5FA9` | 100 | One schema per event type per env | 100 distinct event types per domain |
| Schema versions per schema | `schemas` / `L-3C443A2A` | 100 | Each published revision | 100 revisions of one event type |
| Event bus policy size | `events` / `L-FC354966` | 10,240 chars | **Does not grow**—one fixed statement per bus (`allow-platform-subscription-delivery` on app buses; one producer grant on platform buses) | Not a concern. This is a deliberate design property of resource-policy delivery: per-subscription IAM grants would have grown this policy linearly. |
| Targets per rule | `events` (fixed) | 5 | Konductor attaches exactly 1 target per subscription rule | Not a concern |

## Data-plane throughput (traffic, not counts)

Both throughput quotas apply per account per Region. The platform account therefore aggregates traffic for the entire organization. The following defaults apply to Asia Pacific (Sydney), `ap-southeast-2`. Check the values for your Region. TPS means transactions per second.

| Quota | Code | Default (ap-southeast-2) | What consumes it |
|---|---|---|---|
| PutEvents TPS | `events` / `L-9B653E91` | 1,200/s | Cross-bus **delivery**: every event forwarded to a consumer app bus is a PutEvents call made under the platform delivery role, counted against the **platform** account. (Producers publishing into their platform bus consume their **own** account's PutEvents quota.) |
| Invocations TPS | `events` / `L-5540C5E3` | 2,250/s | Every rule match on a platform bus. An event matched by M subscriptions = M invocations. |

Rule of thumb: total delivered events/sec across **all** domains and environments must fit inside the platform account's PutEvents quota. At 100 domains, if each averages just 15 delivered events/sec, you're at the default ceiling. Both quotas are adjustable—size them from your aggregate expected event rate with peak headroom, and revisit whenever a high-volume domain onboards.

### Metadata pointer reads (admission hot key)

Platform metadata composition adds one strongly consistent DynamoDB point
read to **every admission**: the environment pointer item in
`konductor-metadata-configuration` (`PK=ENV`, `SK=<environment>`). Because
the partition key is the constant `ENV`, **all** environment pointers live
on the same DynamoDB partition, so the partition's documented sustained read
throughput is a single ceiling shared by every environment's admissions. The
following model uses read capacity units (RCUs).

| Quantity | Value | Basis |
|---|---|---|
| RCU per admission | 1 | Strongly consistent read of an item ≤ 4 KB |
| Read ceiling, shared across environments | **3,000 admissions/s** | Documented 3,000 RCU/s per partition; every pointer item sits on the one `ENV` partition |
| Modeled sustained rate (assumption) | 50 events/s per environment (150/s at three environments) | 20× headroom under the shared ceiling |
| Modeled peak rate (assumption) | 500 events/s per environment (1,500/s at three environments) | 2× headroom under the shared ceiling |

A genuinely per-environment ceiling would require a per-environment
partition key (or pointer sharding)—a schema change to the table, not a
configuration knob; revisit if aggregate admissions approach the ceiling.

Reads above the ceiling throttle, and admission **fails closed** (503)—by
design, never replaced by a time to live (TTL) pointer cache. The sustained and peak rows are
named modeling assumptions, not measurements: the repeatable in-Region
harness (`metadata-benchmark-stack.yaml`, deploy/invoke/teardown is
separately authorized) samples the exact production read path
(`MetadataStore::get_environment_pointer`) and replaces them with measured
p50/p95/p99. The closed-form model itself is unit-tested in the
`metadata-benchmark` crate (`cargo test -p metadata-benchmark`).

**Activation cold-start burst.** After an activation advances the pointer's
generation, the first publish of each schema refreshes its registry entry
once. The burst is one `DescribeSchema` per schema in the registry (40 at the
modeled catalog size), so the schema count bounds it and event rate never
multiplies it. Confirm the schemas API quota in your Region before activating
a large catalog; the harness's `registry-describe-burst` probe measures the
real burst over the environment's actual schema list.

## Control-plane throttling during provisioning

These are TPS limits, not counts. `provision-domain` fans out over environments with unbounded Map concurrency, so parallel provisioning bursts against them. Every workflow SDK step retries throttling errors with exponential backoff and jitter (throttle-prone CloudWatch/Schemas calls get 5 attempts; standard control-plane calls get 3), so transient throttles are absorbed rather than failing the environment.

| API | Default TPS | Adjustable | Exposure |
|---|---|---|---|
| EventBridge control plane (CreateEventBus, PutPermission, PutRule, …) | 50/s shared | No | Safe for interactive onboarding; batch operations (backfill scripts, mass re-subscription) should pace themselves |
| CloudWatch PutMetricAlarm | **3/s** | Yes (`monitoring` / `L-0720E68F`) | Each domain provisioning fires 2 alarms × E envs in parallel (6 calls). Workflow retries absorb collisions from a few concurrent approvals; raise the quota for sustained batch onboarding. |
| CloudWatch PutDashboard | 10/s | Yes | E parallel calls per provisioning; low risk |
| Schemas CreateRegistry | throttled (429 shares the `TooManyRequestsException` error code with the count limit—read the message) | Not applicable | E parallel calls per provisioning |

If a provisioning still fails after retry exhaustion (sustained throttling, for example, a large batch onboarding), re-approve the domain—provisioning is idempotent after rollback—and consider raising `L-0720E68F` to 10/s for the onboarding window. Note the schemas caveat: a `Schemas.TooManyRequestsException` caused by the **registry count limit** (not rate) will be retried for ~1 minute before failing; the error message distinguishes the two.

## Other platform-account limits (fine by default, listed for completeness)

The following limits usually require no adjustment for initial deployments.

| Service | Quota | Default | Konductor usage |
|---|---|---|---|
| Lambda | Account concurrent executions | 1,000 (new accounts sometimes start lower—verify) | 13 functions share the pool; SQS-triggered handlers scale with command volume |
| Step Functions | Open standard executions | 1,000,000 | One execution per registration/approval/provisioning/subscription operation |
| API Gateway | Account throttle | 10,000 RPS | Template self-throttles the stage to 50 rps / 100 burst |
| DynamoDB | Tables | 2,500 | 2 tables, on-demand |
| IAM | Roles | 1,000 | ~8 platform roles, fixed |
| CloudFormation | Resources per stack | 500 | Core stack ≈ 70 |
| SES (if notifications enabled) | Sandbox: 200 emails/day, 1/s, verified recipients only | Not applicable | ~2–3 emails per registration. Request SES production access before real onboarding, or operators must pre-verify every owner address. |

Domain accounts need **no quota increases** under the one-domain-per-account model. When `environmentAccounts` concentrates several domains' environments into one account, apply the per-environment counts above to that account before onboarding.

## Cost scaling (no quota, but grows with N)

In this model, costs grow linearly with resource counts. The following estimates use illustrative monthly prices in US dollars (USD) for US East (N. Virginia), `us-east-1`. These estimates are not a current price quote.

Before budgeting, check [Amazon CloudWatch pricing](https://aws.amazon.com/cloudwatch/pricing/) and [AWS Systems Manager pricing](https://aws.amazon.com/systems-manager/pricing/).

| Item | Unit cost | 20 domains | 50 domains | 100 domains |
|---|---|---|---|---|
| Dashboards (N × E, first 3 free) | USD 3/month | About USD 171/month | About USD 441/month | About USD 891/month |
| Alarms (2 × N × E + S × E) | USD 0.10/month | USD 12+/month | USD 30+/month | USD 60+/month |
| SSM advanced parameters (N) | USD 0.05/month | USD 1/month | USD 2.50/month | USD 5/month |

Dashboards account for the largest cost in this model. Consider whether each domain and environment needs a separate dashboard, or whether a consolidated or on-demand view would suffice. This remains a future design decision. See the [Decision log](../decisions/README.md).

## Requesting increases

To check quotas, request increases, and track requests, use the following AWS Command Line Interface (AWS CLI) commands:

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

Request increases in the Region where `konductor-core` is deployed. Large increases might require AWS Support review, so request them before onboarding.

## Pre-onboarding checklist

Before an onboarding wave that takes you to **N** total domains (E = envs in the `Environments` parameter):

- [ ] N ≤ 1,000 (the platform ceiling) with headroom—check the `DomainCount` metric or `konductor-domain-count-capacity-alarm` state
- [ ] Schema Registries quota ≥ N × E × 1.2 (`schemas` / `L-85663EFB`)
- [ ] Event buses quota ≥ (N × E + 2) × 1.2 (`events` / `L-658A4FD9`)
- [ ] Rules-per-bus quota reviewed for any producer expecting > ~250 subscriptions (`events` / `L-244521F2`)
- [ ] PutEvents / Invocations TPS sized from aggregate expected event rate (`L-9B653E91`, `L-5540C5E3`)
- [ ] PutMetricAlarm rate raised for sustained batch onboarding (`monitoring` / `L-0720E68F`)—occasional concurrent approvals are absorbed by workflow retries
- [ ] SES production access granted (if notifications enabled)
- [ ] Stale smoke-test registries/buses cleaned up (they consume the same quotas)
