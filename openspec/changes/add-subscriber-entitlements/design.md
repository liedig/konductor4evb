## Context

Subscriptions are currently authorized by matching the caller's AWS account to the consumer domain owner, and schema discovery is globally visible. `add-grant-engine` delivers the bindings table (`nexus-authorization-bindings`), the `DynamoGrantEngine` and `InMemoryGrantEngine` behind the stringly-typed `ResourceAuthorizer` trait, centralized action constants, and OAuth-only platform administration. `add-publisher-apis` consumes that engine for publisher authorization and establishes the flat-verb, stage-based API era. This change adds the second authorization store (consumer-domain entitlements), extends the engine to compose both stores for subscribe decisions, and reshapes subscription and discovery APIs around it. It consumes both prerequisites rather than reopening their decisions.

The change crosses the access-control, subscription, schema-management, API, and EventBridge materialization boundaries. Authorization must remain current after asynchronous API acceptance, overlapping grants must compose additively, and revocation must stop delivery without deleting the consumer domain's subscription contract.

## Goals / Non-Goals

**Goals:**

- Express producer sharing as consumer-domain-scoped entitlements of two additive types (domain-wide, exact-schema) in a dedicated entitlements table.
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

## Decisions

### Entitlements live in a dedicated second table

The entitlements table (`nexus-authorization-entitlements`) is keyed by consumer domain with an entitlement identifier sort key, and carries: entitlement id, consumer domain, producer domain, scope (`DOMAIN` or `SCHEMA`), schema name (when `SCHEMA`), creation metadata, and trusted actor. A producer reverse GSI (`PRODUCER#{producerDomain}`) serves outbound producer views and producer-scoped administration lists. Mutations follow the bindings-table conventions: conditional writes (idempotent create via not-exists, revoke conditioned on current existence) and one `TransactWriteItems` commit per mutation carrying its token-free audit record. Revocation deletes the entitlement row; history lives only in audit records, so "current coverage" is always a simple existence check.

Both entitlement types cover future artifacts: domain-wide covers schemas created later in the producer domain, and exact-schema covers versions published later under that schema. Entitlements are environment-agnostic; the catalog expands them per environment at read time. Coverage is additive: a subscribe decision passes when ANY current entitlement covers the target schema, never tied to the specific grant that motivated creation.

A separate table (rather than entitlement items in the bindings table) keeps two independently evolving access patterns — identity-keyed lookups versus consumer/producer-keyed coverage queries — off each other's hot partitions, and lets each store grow its own GSIs. This mirrors the settled grant-engine split.

### Subscribe authorization composes inside the engine via a composite resource grammar

The `ResourceAuthorizer` trait keeps its settled stringly shape. Subscribe-family actions carry a composite resource built by typed constructors in `nexus-common` next to the action constants:

```
subscription_resource(consumer, producer, event) -> "{consumer}/{producer}/{event}"
```

Domain names cannot contain `/`, so the engine parses exactly three segments; anything malformed denies (fail closed). The engine maps actions to checks:

- `subscription:create`, `subscription:enable` → binding check (caller holds `subscriber` on consumer domain) AND coverage check (any current entitlement covers `{consumer, producer, event}`)
- `subscription:disable`, `subscription:delete` → binding check only, so subscribers retain lifecycle control after revocation
- Self-subscription (consumer equals producer) → binding check only; a domain is implicitly entitled to its own schemas

The composition lives inside `DynamoGrantEngine` and `InMemoryGrantEngine` so every caller gets both checks from one call, with one audit and deny-log point. The in-memory twin stores fixture entitlements alongside fixture bindings, and the shared fixture suite extends to prove identical two-table decisions on both implementations. A handler calling a separate entitlement store was rejected: the composition would leak into every consumer and forgetting a check would fail open.

### Entitlement administration is flat-verb, OAuth-only, with two authorization paths

Routes on the OAuth surface only: `POST /entitlements`, `GET /entitlements?consumerDomain=|producerDomain=`, `DELETE /entitlements/{entitlementId}`. Authorization:

- Create/revoke: `platform-admin` global role, OR a `publisher` binding on the entitlement's producer domain (producer self-service sharing)
- List: the above, plus a `subscriber` binding matching the queried consumer domain

Administration authorizes through the same engine (a publisher binding check on the producer domain); `platform-admin` resolves from controlled Cognito groups outside grant storage, exactly as in binding administration. The IAM surface gets no entitlement administration in this change; the normalized-principal design makes the extension mechanical later. Consumer-request/producer-approve workflow was rejected as over-built for the dev stage.

### Revocation denies synchronously and disables asynchronously through an explicit queue

`DELETE /entitlements/{entitlementId}` commits the conditional delete plus audit synchronously; every subsequent authorize call denies uncovered actions immediately. The admin handler then sends a revocation message (entitlement scope, consumer domain, revoked-by) to a dedicated SQS revocation queue with its own DLQ.

The worker Lambda processes messages in bounded batches: query candidate active subscriptions through a new producer reverse index on the event-streams table (`PRODUCER#{producerDomain}`, sort key carrying schema name so exact-schema revocations narrow further), re-evaluate each candidate's producer schema against the consumer domain's current grants through the engine, and where uncovered, disable the Nexus-managed rule and per-environment desired state in every environment. Rule identities, pinned versions, and subscription records are retained.

The worker persists per-subscription progress durably; a revocation is not complete until every candidate is confirmed covered, disabled, or recorded in a durable failure state for retry. Retries re-read current grants (a newly added alternative grant legitimately cancels the disable) and disablement is idempotent. This owns the spike's deferred discover fan-out completion bound through bounded batches plus durable progress rather than a fixed seconds SLO.

DynamoDB Streams as the trigger was rejected: an explicit queue message carries the full revoked scope (needed before the row disappears), matches the established command-queue pattern, and keeps the worker's trigger observable and replayable.

### The catalog is an entitlement expansion, never a materialized table

`GET /schemas?consumerDomain={domain}` (plus optional producer-domain and environment filters and opaque pagination) reads the consumer domain's current entitlements, expands domain-wide grants by listing the producer's per-environment registries, looks up exact-schema grants directly, deduplicates overlapping coverage, and reports the latest version per environment so skew stays visible. `GET /schemas/{schemaName}?consumerDomain={domain}&domain={producerDomain}` returns full latest content per environment under the same coverage check. Every page re-evaluates current entitlements; continuation tokens are cursors, not authorization snapshots. A consumer domain with no entitlements returns an empty list — enumeration reveals nothing about what exists.

`platform-admin` and `platform-auditor` omit `consumerDomain` and receive the platform-wide catalog for inspection; the auditor remains read-only everywhere. Registries remain the source of truth; a catalog table was rejected because it would duplicate registry state and break the future-schema coverage rule.

### Subscription APIs become flat verbs with 403 denial

Write routes (both surfaces, direct API Gateway-to-SQS, `202 Accepted`): `POST /subscriptions`, `POST /subscriptions/state`, `DELETE /subscriptions/{subscriptionId}`. `consumerDomain` travels in request bodies alongside the producer schema identity and per-environment directives. The API Gateway mapping injects the trusted-principal envelope (OAuth groups or IAM account) unchanged; the command handler performs the engine decision before any workflow starts, because a queued command may execute after revocation.

Read routes: `GET /subscriptions?domain={domain}&direction=inbound|outbound` and canonical `GET /subscriptions/{subscriptionId}`. Inbound lists a consumer domain's subscriptions under a `subscriber` binding; outbound lists subscriptions consuming a producer domain's schemas under a `publisher` binding, with full visibility because producer teams need consumer topology for notifications and future version migration. Detail authorizes against either visible side or a platform role.

Unauthorized calls return `403 Forbidden`; deny-as-unknown concealment is dropped, accepting that 403 versus 404 reveals existence at the dev stage. List filtering still returns only entries the caller's side is authorized for. The `IamOnlyAuthorizer` bridge is deleted once command paths move to the engine.

## Risks / Trade-offs

- **[Active subscriptions keep flowing until the revocation worker lands]** → Decision-time denial is immediate (sync delete); rule disablement is eventual and measured through worker metrics and per-subscription progress records.
- **[Revocation fan-out can be large]** → Bounded batches through the producer reverse index, idempotent disablement, durable per-subscription outcomes, and the spike's completion-bound recommendation.
- **[A revoked grant can overlap activation]** → Final entitlement check in the command handler and activation path before any rule reaches `ENABLED`; the worker re-evaluates current grants on retry.
- **[Asynchronous callers receive `202` before authorization failure]** → Preserve the established contract and emit deterministic `*Failed` outcome events without retrying business failures.
- **[403 responses reduce concealment]** → Accepted per the dropped deny-as-unknown decision; internal deny reasons are logged without tokens, claims, or payloads.
- **[Authorization can change during a catalog page sequence]** → Re-evaluate grants on every request; cursors carry no authorization state.
- **[Breaking route cutover affects existing clients]** → One coordinated OpenAPI cutover rather than two conflicting ownership models.

## Migration Plan

1. Complete `add-grant-engine` and `add-publisher-apis` (bindings table, engines, publisher bindings, flat-verb schema writes).
2. Deploy the entitlements table, engine extension, and fixture parity without route changes.
3. Deploy entitlement administration; platform admins seed subscriber bindings and entitlements for consumer domains that must retain access (existing account-ownership maps to an AWS-account subscriber binding plus explicit entitlements).
4. Deploy the revocation worker and re-evaluate existing active subscriptions; disable those with no covering grant and durably record partial failures for retry.
5. Deploy the catalog and flat-verb subscription read/write handlers; cut both surface contracts over and remove the global list route and path-scoped routes.
6. Monitor authorization denials, revocation progress and durable failures, disabled-rule outcomes, command failures, and catalog latency.

Rollback SHALL preserve entitlements and any subscriptions disabled for lack of coverage, and SHALL NOT bulk re-enable rules or restore account-ownership authorization without an explicit security review.

## Open Questions

None. Engine composition, administration ownership and surfaces, revocation transport, catalog visibility, and granularity are settled. IAM-surface entitlement administration is a noted follow-up, not an open design question.
