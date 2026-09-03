# Publishing Events

How producers publish events to a Konductor domain: get authorized, register a schema, then publish with store-and-forward durability. You never name an environment, registry, or event bus — the API stage selects the environment and Konductor derives everything else.

This guide assumes your domain is `ACTIVE` (see [Domain Management Guide](domain-management.md)) and uses `awscurl` against the IAM surface, as the [Domain Management Guide](domain-management.md) does. The OAuth surface exposes the same operations with the same semantics for interactive callers (see [OAuth Authentication Guide](development/oauth-authentication-guide.md)).

## Overview

Publishing is a single API call:

```text
POST /{stage}/publish   { "domain", "eventName", "data", "idempotencyKey"? }
        │
        ▼  synchronous admission: authorize → domain ACTIVE →
        │  resolve latest schema → validate data against it
        ▼  202 Accepted (only when every check passed)
   SQS queue  ── durable from this point ──
        ▼
   Konductor delivery: stamp canonical envelope → PutEvents to the domain's event bus
```

You hand Konductor a domain, an event name, and your business data. Before answering, Konductor authorizes the call against your domain's publisher bindings, confirms the domain is `ACTIVE`, resolves the latest schema version in the target environment, and validates your `data` against it. `202 Accepted` means all of that passed and the event is durably queued — delivery and retries are the platform's problem from that point. Failures answer synchronously instead: `403` (no publisher binding), `404` (domain or schema missing), `422` (payload violates the schema, violations listed).

## Before you can publish: publisher bindings

Publishing and schema writes require a **`publisher` role binding** on the domain. A binding grants a principal — your AWS account (IAM surface) or a Cognito team group (OAuth surface) — the publisher role for one specific domain. Bindings are domain-isolated: a binding for `orders` authorizes nothing on `payments`.

Bindings are granted by platform administrators through the binding administration API (`POST /bindings`, OAuth-only). Without one, schema writes and publishes return `403 Forbidden`. If that happens, ask your platform team for a publisher binding naming your domain and your principal (AWS account ID or team group). Administrators: see [Bootstrapping Role Bindings](development/oauth-authentication-guide.md#bootstrapping-role-bindings).

One binding covers every environment stage — you do not need per-environment grants.

## Step 1: Choose your environment (the API stage)

The API Gateway stage **is** the environment selector:

```text
https://{api-id}.execute-api.{region}.amazonaws.com/dev/publish      → dev
https://{api-id}.execute-api.{region}.amazonaws.com/staging/publish  → staging
https://{api-id}.execute-api.{region}.amazonaws.com/prod/publish     → prod
```

There is no environment field in any request body. The same call on different stages targets that environment's schema registry and event bus independently. Environment-agnostic operations (register, list, describe, decide) behave identically on any stage.

## Step 2: Register your schema

Every event needs a schema in the target environment's registry before it can be published there. Create version 1:

```bash
awscurl -X POST "${API_URL}/schemas" --region ${AWS_REGION} \
  -H "Content-Type: application/json" \
  -d '{
    "domain": "orders",
    "eventName": "OrderNotification",
    "schemaType": "JSONSchemaDraft4",
    "content": "{\"$schema\":\"http://json-schema.org/draft-04/schema#\",\"type\":\"object\",\"properties\":{\"orderId\":{\"type\":\"string\"}}}"
  }'
```

```json
{ "schemaName": "acme-orders@OrderNotification", "schemaVersion": 1 }
```

Key things to know:

- **Schemas are JSON Schema draft-4.** Any other `schemaType` answers `400`, and `content` that does not compile as a draft-4 document is rejected (`400`) before anything is written — so a schema you successfully registered can always be validated against at publish time.
- **Konductor owns the schema name.** It is always `{org}-{domain}@{eventName}`, derived from the domain's registered namespace identity. You never supply a schema name, version, or registry.
- **Event names are validated up front.** Characters are limited to `[A-Za-z0-9._-]`, and the name must be able to form a valid subscription rule name within EventBridge's 64-character limit. Invalid names fail with `400` before anything is written.
- **Duplicates are rejected, never replaced.** Creating an existing schema returns `409 Conflict`; the existing schema and all its versions remain unchanged.
- **The domain must be `ACTIVE`.** Otherwise `404 Not Found`, and no registry write occurs.

Evolve an event by **appending** a version:

```bash
awscurl -X POST "${API_URL}/schemas/versions" --region ${AWS_REGION} \
  -H "Content-Type: application/json" \
  -d '{ "domain": "orders", "eventName": "OrderNotification", "content": "{ ... }" }'
```

This returns `201` with the next version number (`404` if the schema does not exist in that environment). Schemas are **append-only**: there is no delete, retire, replace, or version deletion — earlier versions remain immutable so existing consumers keep a valid contract.

## Step 3: Publish an event

```bash
awscurl -X POST "${API_URL}/publish" --region ${AWS_REGION} \
  -H "Content-Type: application/json" \
  -d '{
    "domain": "orders",
    "eventName": "OrderNotification",
    "data": { "orderId": "o-123", "amount": 99.50 },
    "idempotencyKey": "order-o-123-created"
  }'
```

```json
{ "requestId": "493f4be1-...", "message": "Request accepted" }
```

The body accepts exactly `domain`, `eventName`, `data`, and an optional `idempotencyKey` — anything else is rejected. Konductor derives the rest and delivers this envelope to the domain's bus:

```json
{
  "source": "acme-orders",
  "detail-type": "OrderNotification",
  "detail": {
    "metadata": {
      "schemaVersion": 1,
      "idempotencyKey": "order-o-123-created",
      "publishedAt": "2026-08-05T20:58:35.840Z"
    },
    "data": { "orderId": "o-123", "amount": 99.50 }
  }
}
```

- `source` is the domain's environment-independent namespace identity — identical in every environment.
- `metadata.schemaVersion` is the registry's **latest** version at admission time — the exact version your payload was validated against. You cannot choose a version.
- `metadata.idempotencyKey` is your key, or the API Gateway request ID if you omit it.
- `metadata.publishedAt` is when the platform accepted the event (the queue timestamp), not when it was delivered.

Konductor validates your `data` against the schema's latest version **before** accepting the publish. A conforming payload gets `202`; a violating one gets `422` with the violations listed in `message` (for example, `-5 is less than the minimum of 0; "aud" does not match "^[A-Z]{3}$"`) — fix the payload and retry. Konductor also validates the envelope it generates (identity, numeric version, structure, EventBridge size limits) before delivery.

## Durability, retries, and idempotency

`202 Accepted` is a durability guarantee: the event is in the queue before any downstream system can lose it.

- **Delivery failures retry automatically.** If EventBridge rejects the entry, the delivery consumer reports the message for retry instead of acknowledging it. Retries carry the **same idempotency key and the same `publishedAt` timestamp** — consumers can deduplicate on the key.
- **Retries exhaust to a DLQ.** After the queue's redrive limit, the message lands in the platform's publish DLQ with all attributes intact for operator inspection.
- **Rejected publishes never reach the queue.** Authorization (`403`), a missing or inactive domain or schema (`404`), and payload violations (`422`) are answered synchronously — nothing is queued, so there is nothing to retry. Fix the cause and republish.
- **The caller does not learn the delivery outcome.** `202` covers admission and durable queueing; if you need delivery confirmation, subscribe to your own domain's bus and observe your events.

## Error reference

| Response | Cause | Action |
|----------|-------|--------|
| `400` | Body failed structural validation (missing field, unknown field, invalid event name); on schema writes, also `content` that is not a valid draft-4 document | Fix the request body / schema document |
| `403` | No `publisher` binding for this principal on this domain | Ask your platform team for a binding |
| `404` | Domain missing/inactive, or no schema registered for this event in this environment | Check domain status; create the schema first |
| `409` (create schema) | Schema already exists in this environment | Append a version instead |
| `422` (publish) | `data` violates the schema's latest version — violations are listed in `message` | Fix the payload and retry; if you just appended a schema version, a plain retry picks it up |
| `503` (publish) | The platform could not evaluate the publish (schema registry or queue trouble) | Retry with backoff; alert your platform team if it persists |
| `202` then nothing arrives | Rare: the domain was disabled between admission and delivery, or EventBridge delivery is failing | The platform's publish DLQ holds exhausted messages — contact your platform team |

## Discovering schemas

`GET /schemas?consumerDomain={domain}` is the subscriber catalog: it expands that consumer domain's entitlements into the covered schemas with the latest version per environment — every listed entry is currently subscribable. `GET /schemas/{schemaName}?consumerDomain={domain}&domain={producer}` returns detail including content; uncovered targets answer `403`. Platform roles omit `consumerDomain` for the platform-wide view. See [Domain Management Guide — Discovering schemas](domain-management.md#discovering-schemas).

## Deep dive

Why the publish path is built this way — the synchronous-failure rationale, the schema cache and refresh-before-reject rule, the earned-stamp invariant, and the alternatives that were rejected — is documented in [Publish Validation at Ingest](development/publish-validation.md). Lower-level internals (principal envelope, retry semantics, grant-engine decision model, least-privilege infrastructure) are in [Architecture](development/architecture.md): *Event Publishing (Validate-at-Ingest)*, *Schema Management (Append-Only)*, *The Canonical Event Envelope*, and *Grant Engine*.
