# Design: add-event-subscriptions

## Context

Domain provisioning already creates the endpoints of every future event stream: a platform-side domain bus (with PutEvents restricted to the owner account), a per-domain schema registry, a subscription DLQ, and an app-side domain bus in the consumer's account. What is missing is the middle: a governed way to connect a consumer domain to a producer domain's events. Today that would require hand-crafted EventBridge rules and cross-account policies — the exact toil Konductor exists to remove.

This change introduces subscriptions as **contracts, not plumbing**: a consumer subscribes to a schema in the producer's registry (always the latest version), and Konductor materializes the event stream (rules, filters, DLQ wiring) in every environment. Two principles from the platform's design philosophy shape everything below:

1. **One representation of an event at any point in time.** Events are business contracts, not software dependency graphs. Subscribers do not choose versions; they bind to the current contract, and version pinning exists to make transitions observable and bounded — not to let consumers live on old versions indefinitely.
2. **Strict platform/app segregation.** The platform owns everything up to and including delivery onto the consumer's app-side bus. What happens after that (integration rules, targets, orphan cleanup) is the application team's responsibility.

Constraints inherited from the existing system: writes are async (API Gateway → SQS → command handler), workflows are Step Functions with SDK integrations and saga compensation, DynamoDB is the system of record with table-per-entity, domain names are the sole identifiers, and all per-domain resources are provisioned per environment from the platform `Environments` list.

## Goals / Non-Goals

**Goals:**

- Subscribe a consumer domain to a producer domain's schema with one API call; events flowing in every environment without the consumer touching any AWS resource
- Unsubscribe cleanly: platform-side stream removed, record retained 90 days for audit
- Per-environment enable/disable of a subscription (e.g., dev/test active, prod paused), settable at subscribe time and afterwards
- The `konductor-event-streams` table answers all topology questions — subscribers per schema (and per version), inbound streams per consumer — without enumerating AWS resources
- Rule patterns pin the schema version per environment, making "who consumes v3?" an enforceable governance input for the future evolution thread
- Read-only schema discovery so consumers can find contracts to subscribe to
- A canonical event envelope contract that the future publish capability stamps and validates

**Non-Goals:**

- Schema registration, evolution, versioning governance, or compatibility analysis (separate thread; this design deliberately creates its enforcement substrate)
- Per-env version skew *policy* and stale-enable semantics (follow-ups owned by the evolution thread)
- Schema registry RBAC / visibility restriction (parked until the subscription model is proven)
- Custom per-subscription event patterns — the platform delivers the whole schema stream; fine-grained filtering is the app team's job via local rules
- Non-domain subscribers (arbitrary buses/accounts), SNS/SQS/Kinesis targets, and the Publish API

## Decisions

### D1: Subscribers are registered domains, not arbitrary endpoints

`Subscribe(consumerDomain, producerDomain, schemaName)` — three parameters, two of which are already-validated platform entities. The BRD's original `SubscribeToEvent(domainId, eventType, consumerAccountId, consumerEventBus, eventPattern)` shape is rejected.

**Why:** the consumer's app-side bus already exists per env with known naming; the consumer's identity, ownership, DLQs, and monitoring come for free; and there is no foreign-bus validation problem. Arbitrary-bus subscribers can be added later as an escape hatch without disturbing this model.

### D2: Subscription binds to the latest schema version, pinned per environment

At subscribe time, Konductor resolves the **latest** version of the schema in each environment's registry and pins it in that env's rule pattern. Subscribers cannot select historical versions.

**Why:** the schema is the contract; offering old versions would institutionalize multiple concurrent representations of the same event. Pinning (rather than matching any version) makes the consumer population per version *observable and enforceable* — the substrate the evolution thread needs for managed transitions and grace periods.

**Consequence (accepted deliberately):** if a producer starts publishing v4 while a subscriber's rule pins v3, that subscriber's stream silently stops (unmatched events do not DLQ — they are simply not delivered). This is the enforcement teeth of contract pinning; the evolution thread must pair it with transition governance and "zero events delivered" alarming. Per-env version skew (dev at v5, prod at v3) is recorded faithfully in the streams table rather than hidden.

**Alternative considered:** matching a version *range* (`numeric <=`) — rejected for v1 as it reintroduces multiple live representations; the numeric stamping decision (D5) keeps this door open if evolution needs it.

### D3: Delivery authorization is a provision-time resource policy, not per-subscription IAM roles

The consumer's app-side bus gets a resource policy — attached once, during `provision-env`, via the trust role's existing `events:PutPermission` grant — allowing the **platform account** to `PutEvents`. Subscription rules then target the consumer bus directly with the platform's delivery role. The previously specced (never implemented) per-subscription bus-to-bus assumed roles are retired. **BREAKING** at spec level only.

**Why:** the platform account is the *only* cross-account sender any app-side bus will ever receive from, so per-subscription grants add ceremony without adding security. This collapses the subscribe saga to platform-account-only operations — no trust-role assumption at subscribe time, no consumer-account mutation, simpler compensation — and unsubscribe cannot strand IAM roles.

**Trade-off:** an account-level grant is coarser than per-subscription roles — any principal in the platform account with `events:PutEvents` could write to app buses. Accepted: the platform account is the trusted control plane by definition; its IAM posture is the real boundary.

**Alternatives considered:** per-subscription b2b roles in the platform account (per the old spec) — rejected: role proliferation, shared-role cleanup accounting on unsubscribe, no added protection. B2b roles in the *domain* account — rejected: cross-account rule targets require the role in the account owning the rule.

### D4: One rule per subscription, named `{consumerDomain}-{detailType}`, version-agnostic

Each subscription materializes as exactly one rule per environment on the producer's platform-side bus. The name omits source (implied by the bus) and version (lives in the pattern and the record, never the name).

**Why the name is load-bearing:** on a given producer bus, (consumer, detailType) *is* the subscription identity — so EventBridge's per-bus rule-name uniqueness becomes free duplicate-subscription enforcement, and `PutRule` becomes a natural upsert: retried saga steps and future version migrations are the same idempotent call against the same name. Version-agnostic naming means migrating a subscriber v3→v4 is an atomic pattern update — no delete/create window, no metric or DLQ-config discontinuity.

**Constraint accepted:** rule names cap at 64 chars, charset `[A-Za-z0-9._-]`. v1 validates event names at subscribe time (moving to registration time when schema registration comes in-platform); a deterministic hash-suffix fallback is specified only if real names exceed the budget.

**Alternative considered:** shared rule per event type with up to 5 targets packed — denser against the ~300 rules/bus soft quota, but couples subscriber lifecycles (one consumer's change touches others' delivery path) and breaks per-subscription state/metrics. Rejected; quota is a managed-scaling concern (see Risks).

### D5: Canonical envelope — schema name is `{source}@{detail-type}`, version is numeric in `detail.metadata.schemaVersion`

- `source` = the domain's env-independent **namespace identity** `{org}-{domain}` (e.g., `prezzee-orders`) — derived by the platform from the existing SSM namespace, never free-typed by producers
- `detail-type` = the event name
- schema name = `{source}@{detail-type}` — the native EventBridge schema-discovery convention, so the identity round-trips directly to `aws schemas describe-schema --schema-name {source}@{detail-type} --schema-version N`
- schema version = **numeric** at `detail.metadata.schemaVersion` (top-level `version` is AWS-reserved); `detail` splits into `metadata`/`data`

The resulting rule pattern:

```json
{
  "source": ["prezzee-orders"],
  "detail-type": ["OrderNotification"],
  "detail": { "metadata": { "schemaVersion": [3] } }
}
```

**Why:** one string — the schema name the consumer picked — encodes the entire filter, so the API *cannot express* a subscription that disagrees with the contract being signed. Identity is env-portable (same name and pattern in every env; only registry/bus names carry the env segment). Numeric stamping keeps range-matching available to the evolution thread. **Dependency created knowingly:** the envelope is now a platform-wide contract that the publish capability must stamp and validate; the subscription rules are only as trustworthy as publish-side stamping.

**Alternatives considered:** filtering on schema ARN — rejected: ARNs are env-specific and derivable, adding noise; reverse-DNS source (`com.prezzee.orders`) — rejected: introduces a second identity convention when the platform already owns one; free-text producer-chosen source — rejected: guarantees drift (`prezzee-orders` vs `Prezzee.Orders`).

### D6: `konductor-event-streams` table is the source of intent; rules are materialization

New table (table-per-entity convention). Nobody enumerates rules across N buses to understand topology, and a future reconciliation job can diff table-vs-rules to detect drift.

```text
PK: producerDomain          SK: {schemaName}#{consumerDomain}
GSI by-consumer: consumerDomain → all inbound streams

Item: producerDomain, consumerDomain, source, detailType, schemaName,
      status (PENDING | ACTIVE | FAILED | REMOVED),
      environments: { {env}: { pinnedVersion, ruleName, ruleArn,
                               enabled (desired), status (materialized) } },
      requestId, createdAt, updatedAt, removedAt,
      expiresAt (TTL = removedAt + 90d)
```

**Access patterns:** subscribers of a schema (Query PK + `begins_with(SK, "{schemaName}#")` — the evolution thread's version-census, one query, no filter); consumer's inbound streams (GSI); stream detail (GetItem); org topology (scan — small N). Per-env pinned versions live in one item, so skew is visible at a glance rather than scattered across env-partitioned rows.

**Why one item per subscription (not per env):** a subscription is one contract with per-env materialization — mirroring the domain record's `resourceArns` shape — and single-item reads/writes keep saga bookkeeping and idempotency simple. The 90-day audit retention (BRD) becomes a TTL attribute instead of a cleanup process.

**Subscription identity in the API:** `{consumerDomain}.{producerDomain}.{detailType}` (e.g., `checkout.orders.OrderNotification`). Domain names cannot contain dots, so parsing splits on the first two dots; `detailType` may contain dots safely. No UUIDs, consistent with "the domain name IS the identifier."

### D7: Per-environment enable/disable maps to native rule state

Enablement is a per-env property of a subscription (dev active, prod paused for cost during development), mapped to EventBridge rule `State: ENABLED|DISABLED` — an atomic flip, no teardown. Subscribe accepts initial per-env state (default all-enabled); `PATCH` updates it afterwards. Disabled ≠ removed: the rule exists, the version stays pinned, the contract remains signed — so disabled subscribers still appear in version-census queries (flagged, so evolution governance can weigh active vs. paused). The record tracks `enabled` (desired) separately from `status` (materialized). The same primitive is the operational kill switch for floods/incidents.

### D8: Control-plane shape follows the established write path

- **Writes** (`POST/PATCH/DELETE` on subscriptions): API Gateway → subscriptions SQS command queue (+ DLQ, `HttpMethod` attribute, 202 Accepted) → subscription command handler → validation + record write → Step Functions **subscribe/unsubscribe workflow** → outcome events (`SubscriptionCreated|SubscriptionFailed|SubscriptionRemoved|...`) on the konductor bus.
- **Reads**: dedicated sync Lambdas (`list-subscriptions`, `describe-subscription`, `list-schemas`) per the one-Lambda-per-operation convention.
- **Subscribe workflow** (per env, Map state like `provision-domain`): resolve latest schema version (`schemas:DescribeSchema`) → `PutRule` (pattern + state from desired enablement) → `PutTargets` (consumer app bus, `DeadLetterConfig` → the producer's existing subscription DLQ, retry policy). Compensation: delete rules created so far; **all-or-nothing across envs** — if the schema is missing in any env, the subscription fails entirely (consistent with domain provisioning; schema promotion discipline belongs to the evolution thread).
- **Unsubscribe workflow**: per env `RemoveTargets` → `DeleteRule`, idempotent ("already gone" = no-op), then mark record `REMOVED` + set TTL.
- **Validation at command handling:** both domains `ACTIVE`; caller's account (from IAM auth context, passed through the SQS message) must equal the consumer domain's `ownerAccountId`; event-name/rule-name constraints (D4). Self-subscription (consumer == producer) is allowed — it is how a domain consumes its own events.

**Why Step Functions rather than doing N envs in the handler Lambda:** consistency with the provisioning precedent, native per-env compensation, `.sync` observability, and no Lambda-timeout coupling to env count.

### D9: Schema discovery is a thin read-only façade over the registries

`GET /schemas?domain={producer}` lists schemas by calling `schemas:ListSchemas`/`DescribeSchema` against the producer's per-env registries and merges results keyed by schema name, reporting the latest version per env (making skew visible *before* subscribing). No caching, no table — registries are the source of truth for contracts; the streams table only records what consumers signed. Any authenticated caller may discover (visibility RBAC is the parked thread).

## Risks / Trade-offs

- **Silent stream stop on producer version bump** (pinned rule no longer matches; unmatched events vanish without DLQ) → *by design* as contract enforcement, but mitigation is mandatory: the evolution thread inherits transition governance as a hard requirement, and per-subscription "zero events delivered over threshold" alarms should land with or shortly after this change. Documented in both specs and the follow-ups memory.
- **Publishers bypass the envelope** (until the Publish API exists, producers PutEvents directly and could mis-stamp or omit `detail.metadata.schemaVersion`) → interim: envelope contract documented prominently in the domain-owner guide; permanent: publish-side stamping/validation is specced as the `event-envelope` capability's consumer. Mis-stamped events simply don't match subscriptions — failure is non-delivery, not misdelivery.
- **~300 rules/bus soft quota** caps subscriptions per producer per env → monitor stream counts from the table (cheap query); request quota raises per bus as producers grow; the scale target's "distribute across buses" strategy remains available later without API change (rule placement is a materialization detail the table abstracts).
- **Coarse platform-account PutEvents grant on app buses** → accepted (D3); revisit only if the platform account ever hosts non-control-plane workloads.
- **Table/rules drift** (partial saga failures, manual console edits) → the table is declared source of intent; compensation shrinks the window; a reconciliation job (diff table vs. rules, report or repair) is noted as follow-up, not required for v1.
- **Pre-change domains lack the delivery policy** — subscriptions targeting such a domain would fail delivery (no resource policy on its app bus) → resolved by descoping: the product is unreleased, no installed base exists, and dev stacks are rebuilt from scratch so every domain is provisioned with the policy. A backfill script was built, then removed: live testing showed operator credentials cannot assume the trust role (it trusts only the platform service roles), so the script could never run as designed, and it had no remaining audience.
- **Per-env version skew recorded, not resolved** — one subscribe call may pin v5 in dev and v3 in prod → faithful recording is the v1 contract; policy belongs to the evolution thread (open follow-up).

## Migration Plan

1. Deploy core stack update: streams table, subscription command queue + DLQ, new Lambdas, subscribe/unsubscribe workflows, `provision-env` gains the app-bus resource-policy step (new domains are subscription-ready at birth).
2. Publish updated OpenAPI + docs; subscription endpoints become usable.
3. **Rollback:** subscription resources are additive — disable the API routes and delete created rules (table records retained). The provisioned resource policies are inert without rules targeting the buses and may remain.

No data migration: the product is unreleased, so no environment carries domains provisioned before the policy step existed (backfill descoped).

## Open Questions

- **Stale-enable semantics** — enabling a long-disabled subscription whose pinned version has fallen behind: warn, require contract re-affirmation, or silently enable? Owned by the evolution thread; the record's per-env pin makes staleness detectable either way.
- **Operator force-disable authority** — should platform operators be able to disable streams they don't own (incident response)? Mechanism exists (D7); the authorization question is deferred.
- **Zero-events alarm placement** — per subscription (precise, more alarms) vs. per producer bus (coarse, fewer). Leaning per-subscription with a sensible default threshold; finalize during tasks.
