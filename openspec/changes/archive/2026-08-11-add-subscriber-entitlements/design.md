## Context

Subscriptions are currently authorized by matching the caller's AWS account to the consumer domain owner, and schema discovery is globally visible. `add-grant-engine` delivers the bindings table (`konductor-authorization-bindings`), the `DynamoGrantEngine` and `InMemoryGrantEngine` behind the stringly-typed `ResourceAuthorizer` trait, centralized action constants, and OAuth-only platform administration. `add-publisher-apis` consumes that engine for publisher authorization and establishes the flat-verb, stage-based API era. This change adds the second authorization store (consumer-domain entitlements), extends the engine to compose both stores for subscribe decisions, and reshapes subscription and discovery APIs around it. It consumes both prerequisites rather than reopening their decisions.

The change crosses the access-control, subscription, schema-management, API, and EventBridge materialization boundaries. Authorization must remain current after asynchronous API acceptance, overlapping grants must compose additively, and revocation must stop delivery without deleting the consumer domain's subscription contract.

## Goals / Non-Goals

**Goals:**

- Express producer sharing as consumer-domain-scoped entitlements governed by a single additive scope-pattern grammar (domain-wide `*`, exact schema name, or name prefix with a trailing `*`) in a dedicated entitlements table.
- Compose subscriber binding plus covering entitlement inside the grant engine's single `authorize()` call through a composite resource grammar.
- Disable no-longer-entitled active subscriptions while preserving recoverable subscription state, with decision-time denial immediate and rule disablement eventual.
- Provide flat-verb domain-scoped subscription APIs with inbound and outbound views and an entitlement-expanded schema catalog.
- Preserve direct API Gateway-to-SQS subscription writes and `202 Accepted` behavior.

**Non-Goals:**

- Changes to publisher schema creation, append, or publication contracts delivered by `add-publisher-apis`.
- Schema promotion, compatibility, or migration design.
- IAM-surface entitlement administration (noted follow-up; the normalized principal check extends for free).
- Enterprise identity-provider group mapping.
- Management of application-owned rules downstream of a consumer app-side bus.
- Automatic re-enablement of subscriptions after a later grant.
- A durable command-idempotency store (remains deferred; subscription commands are state-idempotent).
- Per-revocation completion tracking. Queue depth, DLQ depth, and reconciliation drift are the operational signals; a progress store is a documented retrofit if revocation volume or compliance ever demands per-revocation propagation proof.

## Decisions

### Entitlements live in a dedicated second table

The entitlements table (`konductor-authorization-entitlements`) is keyed by consumer domain with an entitlement identifier sort key derived deterministically from the (consumer domain, producer domain, scope pattern) triple — which is what makes the conditional not-exists create genuinely idempotent under retries — and carries: entitlement id, consumer domain, producer domain, scope pattern, creation metadata, and trusted actor. A producer reverse GSI (`PRODUCER#{producerDomain}`) serves outbound producer views and producer-scoped administration lists. Mutations follow the bindings-table conventions: conditional writes (idempotent create via not-exists, revoke conditioned on current existence) and one `TransactWriteItems` commit per mutation carrying its token-free audit record. Revocation deletes the entitlement row; history lives only in audit records, so "current coverage" is always a simple existence check.

The scope is a single validated pattern over EventBridge schema names rather than an enum of grant types: `*` covers every schema in the producer domain including schemas created later; an exact name covers all versions of one schema including versions published later; a name prefix with a single trailing `*` covers a family including matching schemas created later. The grammar is deliberately minimal — the schema-name charset with one optional trailing wildcard — and since `*` is not legal in schema names, a pattern can never collide with a literal name. Malformed patterns are rejected at the model boundary before any write, and evaluation fails closed. Domain-wide and exact grants are the same mechanism with degenerate patterns, so the engine, catalog, worker, and audit formatting all share one matcher with no per-type branches. Coverage is additive: a subscribe decision passes when ANY current entitlement's pattern matches the target schema, never tied to the specific grant that motivated creation.

Every pattern covers future artifacts within its match set. Entitlements are environment-agnostic by design: an entitlement answers the contract-level question of whether a consumer domain may use a schema at all, and environments are deployment stages of the same contract — the catalog already surfaces version skew, and per-environment control lives in the subscription (pinned versions, per-environment enablement). Environment-scoped entitlements are rejected: they would complicate the grammar and every evaluation path to serve a use case no tenant has.

Entitlements are intent records; EventBridge rules are their compiled enforcement. Every entitlement read is control-plane: subscribe and enable authorization, the final check before enablement, revocation worker and sweep re-evaluations, catalog expansion, and administration lists. The runtime event path contains no Konductor code — once a rule is `ENABLED`, EventBridge pattern matching routes events with no per-event entitlement evaluation. This is what makes the eventual-consistency posture coherent: revocation flips intent synchronously (the next authorize denies), and enforcement converges through the stream-triggered worker with the sweep as backstop.

Materializing patterns into per-schema entitlement rows (scope splitting) is rejected: it freezes coverage to the schema set at split time, silently breaking future coverage, and no consumer of entitlement data wants per-schema rows — the worker finds candidates through the producer-partitioned event-streams table and re-checks coverage, and the catalog expands against registries. Carve-outs ("keep the domain-wide grant but revoke one schema") are intentionally unsupported: negative scopes would break additivity, and auto-splitting on carve-out would silently change future coverage. The supported escape hatch is revoke plus explicit re-grant of the patterns to keep.

A separate table (rather than entitlement items in the bindings table) keeps two independently evolving access patterns — identity-keyed lookups versus consumer/producer-keyed coverage queries — off each other's hot partitions, and lets each store grow its own GSIs. This mirrors the settled grant-engine split.

### Subscribe authorization composes inside the engine via a composite resource grammar

The `ResourceAuthorizer` trait keeps its settled stringly shape. Subscribe-family actions carry a composite resource built by typed constructors in `konductor-common` next to the action constants:

```
subscription_resource(consumer, producer, event) -> "{consumer}/{producer}/{event}"
```

Domain names cannot contain `/`, so the engine parses exactly three segments; anything malformed denies (fail closed). The engine maps actions to checks:

- `subscription:create`, `subscription:enable` → binding check (caller holds `subscriber` on consumer domain) AND coverage check (any current entitlement covers `{consumer, producer, event}`)
- `subscription:disable`, `subscription:delete` → binding check only, so subscribers retain lifecycle control after revocation
- Self-subscription (consumer equals producer) → binding check only; a domain is implicitly entitled to its own schemas

The composition lives inside `DynamoGrantEngine` and `InMemoryGrantEngine` so every caller gets both checks from one call, with one audit and deny-log point. The in-memory twin stores fixture entitlements alongside fixture bindings, and the shared fixture suite extends to prove identical two-table decisions on both implementations. A handler calling a separate entitlement store was rejected: the composition would leak into every consumer and forgetting a check would fail open.

A composed subscribe decision is two sequential strongly-consistent reads: the binding check (one point read per candidate principal, short-circuiting on first hit) followed by one query on the consumer domain's entitlement partition, with pattern matching in memory. Collapsing the pair was considered and rejected: `BatchGetItem` is eventually consistent and would violate the strong-consistency invariant that grant-then-use and revoke-then-deny rely on; a `TransactGetItems` snapshot across both stores would require re-keying entitlements for point-read coverage and buys nothing end-to-end, because commands execute seconds later and the final entitlement check before enablement already bounds staleness; caching is rejected because revocation must deny on the very next authorize call. Subscribe and enable are low-frequency administrative operations, so two reads at single-digit milliseconds each are immaterial.

### Entitlement administration is flat-verb, OAuth-only, with two authorization paths

Routes on the OAuth surface only: `POST /entitlements`, `GET /entitlements?consumerDomain=|producerDomain=`, `DELETE /entitlements/{entitlementId}`. Authorization:

- Create/revoke: `platform-admin` global role, OR a `publisher` binding on the entitlement's producer domain (producer self-service sharing)
- List: the above, plus a `subscriber` binding matching the queried consumer domain

Administration authorizes through the same engine (a publisher binding check on the producer domain); `platform-admin` resolves from controlled Cognito groups outside grant storage, exactly as in binding administration. The IAM surface gets no entitlement administration in this change; the normalized-principal design makes the extension mechanical later. Consumer-request/producer-approve workflow was rejected as over-built for the dev stage.

### Stream-driven side effects (first application)

This change introduces the project's first DynamoDB stream and with it a standing principle: a handler performs exactly one synchronous commit, and every other side effect of that commit — queue messages, workflow starts, notifications, business events — is driven by the table's stream, whose records are atomic with the commit. Handlers never dual-write. This generalizes what the API edge already practices (API Gateway writes commands directly to SQS): one durable envelope per unit of work.

Constraints that decide where the principle applies:

- The item must carry everything a consumer needs; the stream record is the only message and cannot be enriched with handler-local state.
- No row, no stream record: outcomes about rejections that never committed require a committed state transition (for example a rejected-status row) or remain on an explicit emission path.
- Delivery is at-least-once with per-key ordering; consumers are idempotent on the stream event id.
- Stream latency (seconds) fits the asynchronous command world; synchronous read-your-write paths are exempt.
- Stream retention is 24 hours with shard-level blocking on poison records; consumers need bisect-batch-on-error, retry caps, an on-failure destination, and IteratorAge alarms.
- When the side effect is itself the purpose (event publishing), the durable envelope is the inbound command message, not a stream.

Stream records are change records, not business events. A change record says a row appeared, changed, or disappeared; a business event says something happened in the domain (`EntitlementRevoked`, `RegisterDomainSucceeded`). Where audit value or broader event sharing warrants it, a stream consumer translates the change record into a business event on the control-plane bus, enriching only from durable audit state, never from a deleted row. This keeps auditing at the business level and prevents the stream's change-record shape from leaking into event contracts that may be shared more broadly later. Mechanical reactions (enqueueing worker work) stay at the change-record level; not every record earns a business event.

### Revocation denies synchronously and disables asynchronously through a stream-triggered queue

`DELETE /entitlements/{entitlementId}` commits the conditional delete plus audit synchronously; every subsequent authorize call denies uncovered actions immediately. The commit is the handler's only synchronous step. The entitlements table stream (`OLD_IMAGE`) captures the entitlement `REMOVE` record atomically with the delete, and a thin stream-consumer Lambda translates it: it enqueues a revocation message carrying the full revoked scope from the deleted row's old image onto the dedicated SQS revocation queue with its own DLQ, and emits the `EntitlementRevoked` business event on the control-plane bus. The consumer is idempotent on the stream event id, so at-least-once delivery never duplicates messages or events, and a failed conditional delete produces no record and no downstream work. Because the stream record is part of the commit, revocation notification cannot be lost by a handler failure after the delete; the crash window a handler-side send would leave is closed by construction.

The worker Lambda processes messages in bounded batches: query candidate active subscriptions through the event-streams table's producer partition (the table's `producerDomain` hash key with its `{schemaName}#{consumerDomain}` sort key already is the producer reverse index, so an exact-name revocation narrows candidates by key equality and a prefix revocation by `begins_with`, while a `*` revocation scans the producer partition), re-evaluate each candidate's producer schema against the consumer domain's current grants through the engine, and where uncovered, disable the Konductor-managed rule and per-environment desired state in every environment. Rule identities, pinned versions, and subscription records are retained.

The worker is idempotent and retry-safe, so the queue itself provides the durability: a failed batch redelivers and re-processes every candidate (re-reading current grants, so a newly added alternative grant legitimately cancels the disable), and a permanently failing message lands in the DLQ with an alarm for human redrive. There is no per-revocation progress tracking — completion is observed through queue and DLQ depth, not queried from a store. This owns the spike's deferred discover fan-out completion bound through bounded batches plus idempotent redrive rather than a fixed seconds SLO. The audit record in the entitlements table remains the durable history of the revocation itself.

A scheduled reconciliation sweep (EventBridge Scheduler, hourly) is the backstop. It scans active subscriptions, re-evaluates each against current coverage through the engine, and disables any that are uncovered, reusing the worker's disable logic. The sweep bounds every drift class this design deliberately does not prevent atomically — the activation-overlap residual (a final entitlement check that passes just before a revoke commits, enabling a rule after the worker's scan), rollback-compensation residue in the activation state machine, and out-of-band rule changes — to one sweep interval.

Triggering the worker directly from the stream remains rejected: per-key ordering would let a stalled revocation block later entitlement changes for the same consumer domain, 24-hour stream retention is tighter than the queue's, and heavy fan-out belongs on the observable, replayable queue that matches the established command pattern. The stream's role is limited to crash-safe notification and business-event translation.

### The catalog is an entitlement expansion, never a materialized table

`GET /schemas?consumerDomain={domain}` (plus optional producer-domain and environment filters and opaque pagination) reads the consumer domain's current entitlements, expands each scope pattern against the producer's per-environment registries (registry listings filtered by the pattern, exact names looked up directly), deduplicates overlapping coverage, and reports the latest version per environment so skew stays visible. `GET /schemas/{schemaName}?consumerDomain={domain}&domain={producerDomain}` returns full latest content per environment under the same coverage check. Every page re-evaluates current entitlements; continuation tokens are cursors, not authorization snapshots. A consumer domain with no entitlements returns an empty list — enumeration reveals nothing about what exists.

`platform-admin` and `platform-auditor` omit `consumerDomain` and receive the platform-wide catalog for inspection; the auditor remains read-only everywhere. Registries remain the source of truth; a catalog table was rejected because it would duplicate registry state and break the future-schema coverage rule.

### Catalog pagination is a watermark over a re-evaluated union

The catalog cursor is an opaque encoding of the last emitted (producer domain, schema name) plus a hash of the filter set; it carries no per-source vendor tokens and no authorization state. Every page re-reads the caller's current entitlements (consumer path) or the domains table (platform path), enumerates covered sources in producer-domain order, and lists registries only from the watermark forward, stopping as soon as the page fills plus one look-ahead entry. Entries sort by (producer domain, schema name); overlapping coverage deduplicates in the merge; per-entry latest versions are fetched with bounded parallel `DescribeSchema` calls per environment. Page size is capped at 50, and a cursor whose filter hash no longer matches fails with `400 Bad Request`.

Composite cursors built from EventBridge `ListSchemas` tokens were rejected: the API has no documented ordering and no key-based resumption, so sources cannot be merged without full re-reads, and tokens can expire between pages, killing the cursor. A watermark survives any gap and any registry change.

Page-sequence semantics the contract tests pin:

- Guaranteed: stable (producer domain, schema name) ordering; no duplicates within a sequence; revoked coverage never appears in a later page; cursors remain valid indefinitely.
- Not guaranteed: snapshot consistency. Grants and registry contents are re-read per page; schemas created or granted mid-sequence that sort at or before the watermark do not appear in the remaining sequence, though they appear in fresh listings.

Accepted limits, sized by the 1,000-domain deployment cap:

- Within a single registry, listing cannot resume at a key, so later pages re-scan names up to the watermark; per-domain schema counts in the hundreds keep this bounded, and an oversized domain degrades only its own consumers' later pages linearly.
- Each page fans out to at most 50 × environments `DescribeSchema` calls against a control-plane API whose quota is shared with publishing and schema administration; bounded parallelism with backoff and the catalog-latency alarm keep a catalog crawler from starving sibling paths. Versions are append-only, so a short-TTL describe cache is a safe future optimization if the alarm ever fires.
- The platform-role catalog enumerates at most 1,000 domain sources from the domains table, and per-page cost stays at the few registries needed to fill a page.

### Subscription APIs become flat verbs with 403 denial

Write routes (both surfaces, direct API Gateway-to-SQS, `202 Accepted`): `POST /subscriptions`, `POST /subscriptions/state`, `DELETE /subscriptions/{subscriptionId}`. `consumerDomain` travels in request bodies alongside the producer schema identity and per-environment directives. The API Gateway mapping injects the trusted-principal envelope (OAuth groups or IAM account) unchanged; the command handler performs the engine decision before any workflow starts, because a queued command may execute after revocation.

Read routes: `GET /subscriptions?domain={domain}&direction=inbound|outbound` and canonical `GET /subscriptions/{subscriptionId}`. Inbound lists a consumer domain's subscriptions under a `subscriber` binding; outbound lists subscriptions consuming a producer domain's schemas under a `publisher` binding, with full visibility because producer teams need consumer topology for notifications and future version migration. Detail authorizes against either visible side or a platform role.

Unauthorized calls return `403 Forbidden`; deny-as-unknown concealment is dropped, accepting that 403 versus 404 reveals existence at the dev stage. List filtering still returns only entries the caller's side is authorized for. Subscription command paths move off `IamOnlyAuthorizer` onto the engine. The authorizer itself is not deleted: it is the standing IAM-only guard for platform-operated control-plane paths (domain registration/update, approval and provisioning workflows) — platform teams authenticate and then drive those resources with IAM, while the grant engine models publisher/subscriber resource grants for non-platform owners.

## Risks / Trade-offs

- **[Active subscriptions keep flowing until the revocation worker lands]** → Decision-time denial is immediate (sync delete); rule disablement is eventual and observed through queue, DLQ, and drift metrics.
- **[Stream consumer failure delays revocation notification]** → The stream record is durable with the commit; IteratorAge alarms, bisect-batch-on-error, retry caps, and an on-failure destination bound the delay, and redelivery is idempotent on the stream event id.
- **[Revocation fan-out can be large]** → Bounded batches through the producer-partitioned event-streams table, idempotent disablement, and queue redrive with a DLQ.
- **[A revoked grant can overlap activation]** → The final entitlement check before any rule reaches `ENABLED` bounds the common case; the reconciliation sweep bounds the residual to one sweep interval, with a drift alarm to surface it.
- **[Revocation propagation is not tracked per revocation]** → Accepted at this stage: completion is inferred from queue/DLQ depth and sweep drift rather than a progress store, which is a documented retrofit if revocation volume or compliance ever demands it.
- **[Asynchronous callers receive `202` before authorization failure]** → Preserve the established contract and emit deterministic `*Failed` outcome events without retrying business failures.
- **[403 responses reduce concealment]** → Accepted per the dropped deny-as-unknown decision; internal deny reasons are logged without tokens, claims, or payloads.
- **[Authorization can change during a catalog page sequence]** → Re-evaluate grants on every request; cursors carry no authorization state.
- **[Prefix patterns couple sharing blast radius to schema naming hygiene]** → Prefixes are opt-in: `*` shares everything, an exact name shares one schema, a prefix shares a family. The over-matching caveat is documented, and an unintended match is one entitlement delete away from correction.
- **[Breaking route cutover affects existing clients]** → One coordinated OpenAPI cutover rather than two conflicting ownership models.

## Migration Plan

1. Complete `add-grant-engine` and `add-publisher-apis` (bindings table, engines, publisher bindings, flat-verb schema writes).
2. Deploy the entitlements table, engine extension, and fixture parity without route changes.
3. Deploy entitlement administration; platform admins seed subscriber bindings and entitlements for consumer domains that must retain access (existing account-ownership maps to an AWS-account subscriber binding plus explicit entitlements).
4. Deploy the revocation worker and reconciliation sweep; re-evaluate existing active subscriptions and disable those with no covering grant.
5. Deploy the catalog and flat-verb subscription read/write handlers; cut both surface contracts over and remove the global list route and path-scoped routes.
6. Monitor authorization denials, revocation queue and DLQ depth, disabled-rule outcomes, reconciliation drift, command failures, and catalog latency.

Rollback SHALL preserve entitlements and any subscriptions disabled for lack of coverage, and SHALL NOT bulk re-enable rules or restore account-ownership authorization without an explicit security review.

## Open Questions

None. Engine composition, administration ownership and surfaces, revocation transport and completion posture, catalog visibility, and scope granularity (one pattern grammar covering domain-wide, exact, and prefix grants) are settled. The activation-overlap residual is accepted rather than closed transactionally: the final entitlement check bounds the common case, and the hourly reconciliation sweep bounds the tail — an invariant-enforcing veto at the activation commit was considered and rejected as disproportionate for a rare, low-blast-radius operation at this stage. IAM-surface entitlement administration is a noted follow-up, not an open design question.
