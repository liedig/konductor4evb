# Design: multi-env-domain-provisioning

## Context

The existing provisioning workflow creates one set of resources per approved domain. Which environment those resources belong to is decided at Konductor deploy time via a SAM parameter. With the `konductor-platform-drop-environment` change, Konductor becomes an org-wide singleton — which forces the environment concept to move to where it always belonged: the domain.

The feature: when a domain like `sales` is approved, Konductor provisions a complete resource set for each environment the requester asked for, defaulting to `[dev, staging, prod]`, all within the domain owner's account.

## Decisions

### D1: One domain = one owner account across all its environments

```text
  POST /domains { domainName: "sales", ownerAccountId: "111...", environments: [dev, staging, prod] }
                                                │
                                                ▼
  ┌───────────────────────────────────────────────┐
  │  Application account 111122223333             │
  │                                               │
  │   acme-dev-sales-domain-bus                   │
  │   acme-staging-sales-domain-bus               │
  │   acme-prod-sales-domain-bus                  │
  │   (+ integration DLQs per env)                │
  │                                               │
  │   acme-konductor-trust-role  ◄── single role      │
  │                              for all envs     │
  └───────────────────────────────────────────────┘
```

**Rationale**: keeps the API and mental model simple. The common case — a team's domain provisioned into the same account they already own — is made trivial. Organisations that want dev/prod in separate accounts can still model this by registering two domains (`sales-dev`, `sales-prod`) in the accounts they own; this is ugly, but it's a known escape hatch we can document.

**Trust role implication**: a single application account must host resources for all envs of a domain. The trust role in that account cannot itself be env-scoped or there would be three roles per domain-owner account for one domain. The role is renamed from `${Organization}-${Environment}-konductor-trust-role` to `${Organization}-konductor-trust-role` as part of this change. Its inline IAM policies still scope resources by `{org}-{env}-*` so it can manage per-env domain resources — only the role name loses its env segment. (This rename was originally scoped into `konductor-platform-drop-environment`; pulled forward because multi-env provisioning hard-depends on it.)

**Rejected alternatives**:

- Per-env account IDs in the request. Fits a minority of teams (dev/prod account isolation) but complicates the common case, the API schema, and the trust-stack deployment story (one trust stack per account per domain).
- Polymorphic `environments` field accepting either strings or objects. Extra schema complexity for a v1 that should stay focused.

**Future extension path**: if dev/prod-in-different-accounts becomes a real ask, `environments` can be widened from `string[]` to `(string | { name, accountId })[]` without breaking existing clients.

### D2: Workflow split into three embedded state machines

```text
  Layer                  File                        Responsibility
  ──────                 ────                        ──────────────
  Outer coordinator      provision-domain.asl.yaml   Domain-level setup (namespace + RAM),
                                                     orchestrate per-env provisioning,
                                                     handle all-or-nothing rollback.
  Per-env provisioner    provision-env.asl.yaml      Everything the old monolithic SM did
                                                     for one env, including its own saga
                                                     for within-env partial failure.
  Per-env teardown       deprovision-env.asl.yaml    Idempotent deletion of one env's
                                                     resources. Used by outer rollback
                                                     and (future) DELETE /domains flow.
```

**Why three files, not one with a Map state**:

- Compensation logic in a single file is nested two levels deep: "within env N, compensate the partially-created resources, AND THEN in the outer scope, compensate the fully-succeeded siblings". That produces two different rollback styles in the same file and is a maintenance hazard.
- `deprovision-env` is needed regardless — the `DELETE /domains/{id}` flow has to exist, and it's the same unit as the outer rollback path. Extracting it once gives us the primitive.
- Each of the three files stays within a manageable size and has a single responsibility.

**Trade-off**: three files instead of one, and one extra layer of Step Functions execution per env (outer starts N inners). The extra execution adds ~1-2s of Step Functions overhead per env. Acceptable — provisioning is a rare event.

### D3: Parallel per-env provisioning

Outer workflow uses a `Map` state with `MaxConcurrency: 0` (unbounded). Resource names are env-scoped (e.g., `acme-{env}-sales-*`), so there's no contention between concurrent envs. AWS API rate limits per account are the only real concern; three-to-five envs is well within headroom.

**Rejected**: serial Map. Simpler but slower for no clear win. Provisioning is already asynchronous; the user just sees "provisioning…" longer.

### D4: All-or-nothing semantics

If env[0,1] succeed and env[2] fails:

```text
  Map iteration 0 (dev):      SUCCEEDED
  Map iteration 1 (staging):  SUCCEEDED
  Map iteration 2 (prod):     FAILED (inner saga already cleaned up partial prod state)
                                          │
                                          ▼
  Outer:  domain marked PROVISIONING_FAILED
          Map(parallel) over [dev, staging]:
             └─► StartExecution.sync:2 → deprovision-env
          Namespace SSM parameter:  UNCHANGED
          RAM share association:    UNCHANGED
          Emit DomainProvisioningFailed with failedEnvironment: "prod"
```

**Why all-or-nothing**:

- A domain partially provisioned across `[dev, staging]` but missing `prod` is a confusing operational state. The consumer gets "your domain is ready" with prod missing, or "it failed" with dev and staging silently present.
- The user didn't ask for "whichever envs you can manage"; they asked for a domain with a specific env set. Delivering a subset violates intent.
- Retry semantics are simpler: after fixing the underlying issue, the platform team re-runs the failed provisioning (mechanism TBD — likely "re-approve the registration" or "platform team operator tool", out of scope) against a known-clean state.

**Why namespace stays**: the namespace is the domain's identity within the org, minted at approval time. Discarding it on provisioning failure would force a retry to get a new namespace, which (a) leaks an implementation detail and (b) creates `/namespace/sales-v2` style graveyards in SSM. Keep it; reuse on retry.

### D5: `deprovision-env` failure handling

`deprovision-env` runs during outer rollback. Its own failures are tricky: if the outer workflow can't clean up `dev` after `prod` failed, we're now in a state with dangling `dev` resources that the user thinks don't exist.

**Decision**: `deprovision-env` logs per-step failures to a `compensationErrors` array (same pattern as the existing inner saga). The outer workflow collects these from all sibling teardowns and includes them in the `DomainProvisioningFailed` event. Domain status remains `PROVISIONING_FAILED`. The platform team gets an alert with concrete dangling-resource identifiers and must clean up manually.

This mirrors the current best-effort compensation philosophy. Going further (e.g., marking the domain `PARTIAL_TEARDOWN` and retrying later) is more system than is warranted pre-production.

### D6: `environments` is platform-configured, not requester-supplied

The platform team — not the domain requester — decides which environments exist. A domain owner registering `sales` has no legitimate reason to pick a subset or invent new env names; the org has already decided (say) it runs `dev`, `staging`, and `prod`, and every domain lives in all three.

**Mechanism**:

- Konductor core SAM template declares an `Environments` parameter (type `CommaDelimitedList`, default `dev,staging,prod`). Each entry SHALL match `^[a-z0-9][a-z0-9-]{0,31}$`. Validation happens once at stack deploy via SAM's parameter constraints.
- The domain command handler Lambda receives the list via an `ENVIRONMENTS` environment variable and parses it at cold start.
- On registration, the handler snapshots the current list onto the domain record's `environments` field.
- `RegisterDomainRequest` has NO `environments` field. The API schema is unchanged for requesters.

**Why snapshot rather than re-read on every workflow step**: if the platform team later adds `qa`, existing domains stay at their original set unless a deliberate migration updates them. Prevents surprise re-provisioning of every old domain the next time someone touches it. The domain record is the source of truth for that domain's env set; the SAM parameter is the source of truth for *new* domains' env set.

**Rejected alternatives**:

- Per-request `environments` field. Gives the requester a lever they shouldn't have — env set is governance, not user preference.
- Hard-coded enum `[dev, staging, prod]`. Too restrictive for orgs that legitimately want `qa`, `uat`, `sandbox`.
- Reading the list live on every provisioning step. Introduces the drift problem above.
- Per-domain override via approval workflow (approver edits the list). Deferrable — can be added later without breaking anything, and no current ask justifies it.

### D7: Data model — `resourceArns` becomes a map

Today (flat):

```json
{
  "domainId": "...",
  "resourceArns": {
    "platformEventBusArn": "arn:...",
    "appEventBusArn": "arn:...",
    "schemaRegistryArn": "arn:...",
    "subscriptionDlqArn": "arn:...",
    "integrationDlqArn": "arn:...",
    "dashboardName": "..."
  }
}
```

After:

```json
{
  "domainId": "...",
  "environments": ["dev", "staging", "prod"],
  "resourceArns": {
    "dev": { "platformEventBusArn": "...", "appEventBusArn": "...", ... },
    "staging": { ... },
    "prod": { ... }
  }
}
```

Namespace parameter and RAM share identifiers stay at the top level (not in the map) because they are env-independent.

**DynamoDB view**: DDB handles nested maps natively; no schema change beyond accepting a map type. `DescribeDomain` returns the map as-is.

### D8: Failure event carries `failedEnvironment`

```json
{
  "domainId": "...",
  "workflowExecutionId": "...",
  "failedAt": "...",
  "failedEnvironment": "prod",        // NEW
  "failedStep": "CreateAppEventBus",  // existing
  "errorCode": "AccessDenied",        // existing
  "errorMessage": "...",              // existing
  "compensationErrors": [...]         // existing, now aggregated across all envs
}
```

**Why only one `failedEnvironment`**: with parallel envs we might have multiple fail concurrently. The event reports the **first** failure observed by the outer workflow (the one that tripped all-or-nothing). Subsequent env failures (if any) are absorbed in `compensationErrors`. This keeps the event simple; platform team looks at CloudWatch logs for the fuller picture.

### D9: Notification content

Domain owner gets from the existing notification Lambdas:

- On `DomainProvisioningSucceeded`: "Your domain `sales` is now active across `[dev, staging, prod]`. Resource details: …"
- On `DomainProvisioningFailed`: "We encountered a problem provisioning your domain `sales`. The platform team has been notified and will resolve this issue. You'll receive a further update once it's ready." — **no technical detail, no env mention.**

Notification templates are simple text; updating them is a tasks-level concern.

## Migration

Pre-production — no data migration. Any existing domain records (from dev testing) are wiped along with the rest of change A's rename-induced CloudFormation drop.

## Risks

- **Outer rollback failure cases are annoying.** Saga + saga-of-sagas is harder to reason about than saga. The `compensationErrors` aggregation is best-effort; in a pathological failure (platform-wide IAM outage), a domain can end up `PROVISIONING_FAILED` with half-present resources. Platform team owns cleanup. Acceptable.
- **Parallel API rate limits.** Creating 3+ event buses, schema registries, SQS queues, CloudWatch resources concurrently from a single account hits EventBridge/SQS/CloudWatch limits. Three envs is safe; pushing to 10+ might not be. Max length of 10 in D6 is a soft guardrail.
- **`.sync:2` nested executions add a small cost.** Each outer Map iteration spins up a Step Functions execution. For 3 envs, that's 3 inner + 1 outer = 4 executions per domain registration. Marginal cost; Step Functions billing is per transition.
- **Inner state machine backward compatibility.** The existing single-workflow implementation lives in `domain-management/state-machines/provisioning.asl.yaml`. Extracting inner/outer is a rewrite, not an in-place edit. The existing file's compensation chain is the seed of `deprovision-env`.

## Open Questions

None at design time. Task-level decisions (e.g., exact parameter names, rule-wiring, Step Functions logging config) are deferred to implementation; none of them change the shape of this design.
