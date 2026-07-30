## Context

Schema discovery is currently read-only through Nexus, while domain owners register schemas out-of-band. Event publication has no control-plane API. The access-foundation change introduces separate OAuth and IAM control-plane surfaces plus normalized trusted principals. The authorization-engine spike validates the DynamoDB-backed grant engine design but explicitly does not implement it.

The PRFAQ promises simple `publish`, `subscribe`, and `unsubscribe` API operations. Subscribe and unsubscribe already exist as flat endpoints (`POST /subscriptions`, `DELETE /subscriptions/{id}`). This change adds the missing `publish` endpoint and publisher schema management, completing the PRFAQ vision.

## Goals / Non-Goals

**Goals:**

- Give publishers a flat `POST /publish` endpoint where the domain is in the request body, not the URL path. The caller provides domain, event name, and business data; Nexus handles everything else.
- Guarantee event durability through store-and-forward: API Gateway pushes to SQS (202 Accepted), a consumer Lambda resolves the schema, builds the envelope, and calls PutEvents. If PutEvents fails, SQS retries; the event is never lost once it reaches the queue.
- Extend the canonical envelope `detail.metadata` with `idempotencyKey` and `publishedAt`, both immutable from queue time and surviving EventBridge replays.
- Infer the target environment from the API stage (`dev`, `staging`, `prod`) rather than requiring the caller to specify it per event. The environment travels as an SQS message attribute alongside the trusted-principal envelope.
- Give publishers simple schema management endpoints (`POST /schemas`, `POST /schemas/versions`) that mimic the EventBridge schema API.
- Enforce publisher authorization through the production DynamoDB grant engine before any resource access.
- Expose equivalent publisher routes on the separate OAuth and IAM control-plane surfaces with one resource-decision contract.

**Non-Goals:**

- Schema promotion between environments.
- Business-payload validation against schema content.
- Schema deletion, retirement, version deletion, or historical mutation.
- Schema-change event hook workflows (e.g., notify subscribers of new versions).
- Subscriber catalog or entitlement workflows.
- A general event-ingress Lambda or replacement for API Gateway-to-SQS ingestion on other routes.

## Decisions

### Use flat verb endpoints with domain in the request body

The APIs are:

- `POST /publish`
- `POST /schemas`
- `POST /schemas/versions`

The domain, environment-relevant context, and event details are in the request body. Nested per-domain resource paths (`/domains/{domain}/environments/{env}/events/{eventName}`) were considered and rejected: they create multiple API resources per domain, are operationally unmanageable, and contradict the PRFAQ's promise of simple publish/subscribe/unsubscribe operations.

This matches the existing Nexus pattern: `POST /subscriptions` carries the domain pair in the body, not in the URL.

### Infer environment from the API stage

API Gateway stages are named after platform environments (`dev`, `staging`, `prod`) instead of a static `v1`. The stage IS the environment selector:

```
POST https://api.nexus.com/dev/publish    → targets dev environment
POST https://api.nexus.com/staging/publish → targets staging environment
POST https://api.nexus.com/prod/publish   → targets prod environment
```

One API Gateway deployment, one set of Lambda functions. The stage name is available as `$context.stage` in the VTL mapping template and is injected as an SQS message attribute. The publish consumer Lambda reads it from the message attributes to select the environment-specific registry and bus.

Operations that are environment-agnostic (register, approve, list, describe) work identically on any stage. Operations that already carry environment in the body (subscribe) continue to use the body field. Only publish and schema write operations use the stage for environment inference.

A separate data-plane API was considered and rejected: it adds an API Gateway deployment and splits the surface without adding capability. With no dependent workloads, changing the stage naming is safe.

### Store-and-forward publish via SQS

The publish flow uses the existing Nexus write pattern: API Gateway → SQS → consumer Lambda.

```
POST /prod/publish
  │
  ▼
API Gateway (stage = prod)
  VTL mapping template:
    • injects Environment=prod as SQS message attribute
    • injects trusted-principal envelope as SQS message attributes
    • injects idempotencyKey (caller-supplied or $context.requestId)
    • pushes message body to SQS
  │
  ▼
SQS queue (durable, timestamped)
  SQS assigns:
    • MessageId
    • SentTimestamp (epoch ms)
  │
  ▼
Publish Consumer Lambda
  reads message body: domain, eventName, data
  reads attributes: Environment, SentTimestamp, principal envelope, idempotencyKey
  │
  ├── authorize: publisher binding for domain via grant engine
  ├── derive:  schema name = {namespaceIdentity}@{eventName}
  ├── resolve: {org}-{env}-{domain}-schema-registry → latest version
  ├── target:  {org}-{env}-{domain}-bus
  ├── build envelope:
  │     source:      {namespaceIdentity}
  │     detail-type: {eventName}
  │     detail.metadata.schemaVersion:  resolved version
  │     detail.metadata.idempotencyKey: from message attribute
  │     detail.metadata.publishedAt:    ISO 8601 from SentTimestamp
  │     detail.data:                    caller payload
  │
  └── PutEvents → domain event bus
      ├── success → message deleted from queue
      └── failure → SQS retry (same attributes, same key, same timestamp)
```

Caller receives `202 Accepted` with the SQS message ID as `requestId`. The caller does not learn whether PutEvents succeeded; they know the event is durably queued and will be delivered or retried.

A synchronous publish Lambda was considered and rejected: it sacrifices the store-and-forward guarantee. If PutEvents fails in a sync handler, the event is lost (the caller gets an error but may not retry). With SQS, the event survives PutEvents failures and is retried automatically with the same immutable metadata.

### Extend envelope metadata with idempotencyKey and publishedAt

The canonical envelope `detail` contains two sub-objects:

```json
{
  "source": "acme-orders",
  "detail-type": "OrderNotification",
  "detail": {
    "metadata": {
      "schemaVersion": 4,
      "idempotencyKey": "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
      "publishedAt": "2026-07-30T10:30:00.123Z"
    },
    "data": {
      "orderId": 123,
      "amount": 99.50
    }
  }
}
```

**`metadata.idempotencyKey`** — A unique identifier for this event instance. The caller MAY supply it in the request body. If omitted, the VTL mapping template injects `$context.requestId` (API Gateway's unique request ID). The key is set at API Gateway time, before the message reaches SQS, and is immutable from that point. It survives SQS retries and EventBridge replays. EventBridge's native `id` field cannot be set by the producer; this field provides caller-controlled or platform-generated idempotency within the envelope.

**`metadata.publishedAt`** — The timestamp when the event was first accepted by the platform. It is a copy of the SQS `SentTimestamp` attribute (epoch milliseconds, converted to ISO 8601), read by the consumer Lambda from the SQS event record. It represents the moment the event entered the durable queue, not when the Lambda processed it. It is immutable from queue time and survives retries and replays.

**`metadata.schemaVersion`** — The latest numeric schema version resolved from the target environment's registry at publish time. Unchanged from existing convention.

The existing `detail.metadata.schemaVersion` rule pattern in subscription matching continues to work. The new fields are additive.

### Schema management mimics the EventBridge schema API

Publishers manage schemas through two endpoints that mirror EventBridge's `CreateSchema` and `UpdateSchema` semantics:

**`POST /schemas`** — Create a new schema (like EventBridge `CreateSchema`):
```json
{
  "domain": "orders",
  "eventName": "OrderNotification",
  "schemaType": "JSONSchemaDraft4",
  "content": "{ ... }"
}
```
Nexus derives the canonical schema name `{namespaceIdentity}@{eventName}`, targets the stage-selected environment's registry, and creates version 1. Returns `201 Created` with the schema name and version. Returns `409 Conflict` if the schema already exists.

**`POST /schemas/versions`** — Append a new version (like EventBridge `UpdateSchema`):
```json
{
  "domain": "orders",
  "eventName": "OrderNotification",
  "content": "{ ... }"
}
```
Nexus derives the canonical schema name, targets the stage-selected environment's registry, and creates the next version. Returns `201 Created` with the schema name and new version. Returns `404 Not Found` if the schema does not exist.

Both are synchronous Lambda integrations (not SQS), because schema writes must return the created version to the caller. The environment is inferred from `$context.stage`.

Schemas are append-only: no delete, retire, replace, or version deletion operations are exposed. Registry lifecycle and policy administration remain platform-admin operations.

### Publisher authorization through the grant engine

Every publish and schema write request requires a `publisher` resource-role binding for the target domain. The normalized principal (OAuth team group or IAM AWS account) is checked against the DynamoDB-backed grant engine before any resource access.

The production grant engine implementation is a prerequisite for this change. It can be implemented either:
- As a standalone change (recommended by the spike, cleaner separation), or
- Folded into this change as an initial implementation phase

The publisher-binding administration API (platform-admin creates, lists, revokes bindings) belongs with the grant engine, not with the publisher data-plane APIs. This change consumes the grant engine's decision interface; it does not own the engine's persistence or administration.

### Constrain infrastructure permissions

Publish consumer and schema handler roles receive only:
- Domain-state read access (DynamoDB)
- Authorization-decision access (grant engine)
- AWS Schemas read/write scoped to Nexus-managed registry patterns
- EventBridge `PutEvents` scoped to Nexus-managed bus patterns

Publishers cannot manage registries, buses, policies, rules, subscriber grants, or authorization configuration. Application-level authorization is mandatory because dynamic domain bindings are narrower than deploy-time IAM resource patterns.

## Risks / Trade-offs

- [Store-and-forward means the caller does not learn PutEvents outcome] → The caller receives 202 with a durable queue guarantee. The trade-off is deliberate: durability over synchronous feedback. Callers who need delivery confirmation can subscribe to the domain bus and observe their own events.
- [Environment-named stages change the API URL structure] → No dependent workloads exist; the change is safe. Existing `v1` stage can coexist during transition if needed.
- [The production grant engine is not yet implemented] → Make it a prerequisite. The spike validates the design; a standalone change implements the engine before this change's authorization integration.
- [SQS SentTimestamp is epoch ms, not ISO 8601] → The consumer Lambda converts it. The SQS attribute is the source of truth; the envelope carries the ISO 8601 representation.
- [Caller-supplied idempotency keys may collide] → The key is scoped to the domain and event name. Two different events with the same key are distinct. Within the same domain and event, a duplicate key indicates a retry or duplicate publish; downstream consumers can use it for deduplication.
- [Latest-version resolution adds latency to every publish] → Keep the first implementation uncached for correctness; emit latency metrics so a bounded, invalidatable cache can be evaluated separately.
- [Schema management is synchronous while publish is async] → Schema writes must return the created version; publish does not need to return PutEvents results. Different integration patterns match different durability/feedback requirements.

## Migration Plan

1. Implement the production DynamoDB grant engine (standalone change or initial phase of this change).
2. Change API stage naming from `v1` to environment-named stages on both OAuth and IAM surfaces.
3. Add `POST /publish` route contracts, SQS mapping template with environment/idempotency-key injection, and publish consumer Lambda.
4. Add `POST /schemas` and `POST /schemas/versions` route contracts and synchronous Lambda handlers.
5. Create initial publisher bindings through the grant engine's platform-admin API.
6. Exercise publish durability (SQS retry on PutEvents failure), schema creation/versioning, environment routing, and OAuth/IAM parity in a non-production environment.
7. Remove direct application-team registry administration and generic schema-write access from provisioning policies.

## Open Questions

None. Schema promotion and schema-change event hooks are intentionally deferred to separate changes.
