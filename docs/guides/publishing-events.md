# Publish events

Konductor for Amazon EventBridge validates and queues events before delivery. As a producer, you obtain authorization, register a schema, and publish your data. The API stage selects the environment. Konductor derives the registry and event bus.

This guide assumes your domain is `ACTIVE`. It uses `awscurl` with the AWS Identity and Access Management (IAM) API surface. For domain setup, see the [Domain Management Guide](domains.md). Interactive callers use the same operations through OAuth. For authentication, see the [OAuth Authentication Guide](authentication.md).

## Overview

Publishing uses one API call. Konductor validates the request synchronously, then queues the accepted event in Amazon Simple Queue Service (Amazon SQS) for asynchronous delivery:

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

Supply a domain, an event name, and your business data. Before responding, Konductor checks your publisher binding and confirms that the domain is `ACTIVE`. It then resolves the latest schema version in the target environment and validates `data`.

HTTP `202 Accepted` means these checks passed and the event is durably queued. The platform handles delivery and retries. Validation failures return synchronously: `403` for a missing publisher binding, `404` for a missing domain or schema, and `422` for schema violations.

## Before you can publish: publisher bindings

Publishing and schema writes require a **`publisher` role binding** on the domain. A binding grants that role to a principal: your AWS account or an Amazon Cognito team group. A binding applies to one domain. For example, a binding for `orders` does not authorize access to `payments`.

Bindings are granted by platform administrators through the binding administration API (`POST /bindings`, OAuth-only). Without one, schema writes and publishes return `403 Forbidden`. If that happens, ask your platform team for a publisher binding naming your domain and your principal (AWS account ID or team group). Administrators: see [Bootstrapping Role Bindings](../operations/access-administration.md#bootstrapping-role-bindings).

One binding covers every environment stage—you do not need per-environment grants.

## Step 1: choose your environment (the API stage)

The API Gateway stage **is** the environment selector:

```text
https://{api-id}.execute-api.{region}.amazonaws.com/dev/publish      → dev
https://{api-id}.execute-api.{region}.amazonaws.com/staging/publish  → staging
https://{api-id}.execute-api.{region}.amazonaws.com/prod/publish     → prod
```

There is no environment field in any request body. Publishing works on every stage and targets that environment's event bus independently. Schema writes are different: creation and version appends are accepted only on the **authoring stage** (the first configured environment, `dev` in a default installation); every other stage answers `400 AuthoringEnvironmentOnly`. Later environments receive versions only through promotion (`POST /schemas/promotions` on the target stage). Environment-agnostic operations (register, list, describe, decide) behave identically on any stage.

## Step 2: register your schema

Every event needs a schema in the target environment's registry before it can be published there. Create version 1 on the authoring stage (later environments receive it through promotion):

```bash
awscurl -X POST "${API_URL}/schemas" --region "${AWS_REGION}" \
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

- **Schemas are JSON Schema draft-4.** Any other `schemaType` answers `400`. `content` that does not compile as a draft-4 document is rejected (`400`) before anything is written, so a schema you successfully registered can always be validated against at publish time.
- **Konductor owns the schema name.** It is always `{org}-{domain}@{eventName}`, derived from the domain's registered namespace identity. You never supply a schema name, version, or registry.
- **Konductor validates event names before writing.** Names can contain only `[A-Za-z0-9._-]`. The derived subscription rule name must fit the 64-character limit for Amazon EventBridge. Invalid names return `400` before any write.
- **Duplicates are rejected, never replaced.** Creating an existing schema returns `409 Conflict`; the existing schema and all its versions remain unchanged.
- **The domain must be `ACTIVE`.** Otherwise `404 Not Found`, and no registry write occurs.
- **Authoring stage only.** On any other stage the call fails with `400` and code `AuthoringEnvironmentOnly`, naming the authoring environment, before any registry access. No role or binding bypasses this; use promotion to reach later environments.

Evolve an event by **appending** a version:

```bash
awscurl -X POST "${API_URL}/schemas/versions" --region "${AWS_REGION}" \
  -H "Content-Type: application/json" \
  -d '{ "domain": "orders", "eventName": "OrderNotification", "content": "{ ... }" }'
```

This returns `201` with the next version number (`404` if the schema does not exist in that environment). Schemas are **append-only**: there is no delete, retire, replace, or version deletion—earlier versions remain immutable so existing consumers keep a valid contract.

Appends are also **backward compatible only**. The new document is checked against the current latest version with a bounded proof that every previously accepted payload stays accepted. Anything that visibly restricts the contract—or that the proof cannot establish—fails with `400 Bad Request` naming the relevant paths and reasons. In practice:

- **Allowed:** dropping entries from `required`, widening a `type` (for example `"integer"` to `["integer", "string"]`), growing an `enum`, and loosening or removing constraints (`minimum`, `maxLength`, `pattern`, and so on). You can also add an optional property whose constraint provably accepts every value the predecessor allowed for that name. That means any constraint when the predecessor closed the object with `additionalProperties: false`, and an unconstrained `{}` when it did not.
- **Rejected:** removing a declared property, newly requiring a property, narrowing a `type` or `enum`, raising `minimum`, lowering `maximum`, and flipping `additionalProperties` from `true` to `false`. Constraining a previously unrestricted property name on an open object is rejected too (for example adding `"note": {"type": "string"}` to a schema that accepted any `note` value).
- **Also rejected, as unproven rather than demonstrably breaking:** any change to a document carrying schema-bearing `$ref`s, edits to combinators or `patternProperties`, and `id`/`format` changes. The same applies to numeric comparisons that cannot be made exactly and to documents whose comparison exceeds the gate's work budget. These rejections mean the bounded proof could not establish compatibility, not that a breaking payload was found.

Compatibility is judged against the current latest version only, never historical ones, and the gate is absolute—no flag or role bypasses it. If you need a genuinely breaking change (renaming a field, tightening a constraint), register it as a **new schema under a new event name** and migrate consumers explicitly. That keeps every accepted version of an event safe for consumers validated against its predecessors.

## Promoting a schema to later environments

New versions land only in the authoring environment. When a version is ready for the next environment, **promote** it—promotion replays the missing versions into the target registry under the *same* version numbers, so `v3` means the same contract in every environment. Each promotion covers one hop: the target is the stage you call, the source is its configured predecessor (for `dev, staging, prod`, promoting on the `staging` stage replays from `dev`).

```bash
awscurl -X POST "${API_URL_STAGING}/schemas/promotions" --region "${AWS_REGION}" \
  -H "Content-Type: application/json" \
  -d '{ "domain": "orders", "eventName": "OrderNotification" }'
```

The call answers `202 Accepted` with the in-flight promotion (or `409` naming the promotion already running for that schema and environment—promotion is idempotent, so retrying the same request is safe and never double-writes). Track progress with `GET /schemas/promotions?domain=orders&eventName=OrderNotification` on the same stage; the status moves `VALIDATING → REPLAYING → REPINNING → COMPLETED`. In the console, the producer schema page (**Domains → your domain → schema name**) shows the whole pipeline. It shows per-environment versions with behind counts, a Promote action per target environment, and live promotion progress. It also shows the environments each version has reached and every subscriber's pinned version.

Key things to know:

- **Replay validates before writing.** Promotion first compares the version history both registries share (canonical content comparison). If the target's history diverges from the authoring environment's, the promotion stops as `FAILED` with reason `DIVERGED` and leaves the target untouched—contact the platform team to reconcile before retrying.
- **Failures are resumable.** If a replay fails mid-way (`REPLAY_FAILED`), the versions already written stay in the target; fix the cause and promote again—already-replayed versions are skipped, and per-version retries carry the same idempotency token.
- **Subscribers keep receiving events.** After the replay, Konductor re-pins the target environment's subscription rules to the new version through a widen, drain, narrow sequence. Rules first match both the pinned and the new versions, wait out a drain window (five minutes by default), then settle on the new version. No events are dropped in between, and consumers observe this as their rule filters changing—not as a gap in delivery.
- **The version pin is per environment.** A consumer's rule in `staging` moves only when the promotion into `staging` completes; each environment's pins advance independently.
- **You need the publisher binding.** Promotion requires the `schema:promote` grant on the producer domain (held by publishers); auditors can read promotion status but cannot start one.

## Step 3: publish an event

```bash
awscurl -X POST "${API_URL}/publish" --region "${AWS_REGION}" \
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

The body accepts exactly `domain`, `eventName`, `data`, and an optional `idempotencyKey`—anything else is rejected. Konductor derives the rest and delivers this envelope to the domain's bus:

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

- `source` is the domain's environment-independent namespace identity—identical in every environment.
- `metadata.schemaVersion` is the registry's **latest** version at admission time—the exact version your payload was validated against. You cannot choose a version.
- `metadata.idempotencyKey` is your key, or the API Gateway request ID if you omit it.
- `metadata.publishedAt` is when the platform accepted the event (the queue timestamp), not when it was delivered.

When metadata configuration is active for the environment, Konductor stamps `metadata.metadataRevision` and the fields generated by that revision, such as `eventId` and `geo.region`. The platform supplies these values from trusted context. You cannot set them: a top-level `metadata` key fails structural validation with `400`.

Admission freezes the generated values. Delivery and its retries emit the accepted values, even if a new metadata revision becomes active before delivery.

Konductor validates your `data` against the schema's latest version **before** accepting the publish. A conforming payload gets `202`; a violating one gets `422` with the violations listed in `message` (for example, `-5 is less than the minimum of 0; "aud" does not match "^[A-Z]{3}$"`)—fix the payload and retry. Konductor also validates the envelope it generates (identity, numeric version, structure, EventBridge size limits) before delivery.

## Durability, retries, and idempotency

`202 Accepted` is a durability guarantee: the event is in the queue before any downstream system can lose it.

- **Delivery failures retry automatically.** If EventBridge rejects the entry, the delivery consumer reports the message for retry instead of acknowledging it. Retries carry the **same idempotency key and the same `publishedAt` timestamp**—consumers can deduplicate on the key.
- **Exhausted retries move to a dead-letter queue (DLQ).** After the queue's redrive limit, the platform moves the message to the publish DLQ. All attributes remain intact for operator inspection.
- **Rejected publishes never reach the queue.** Authorization (`403`), a missing or inactive domain or schema (`404`), and payload violations (`422`) are answered synchronously—nothing is queued, so there is nothing to retry. Fix the cause and republish.
- **The caller does not learn the delivery outcome.** `202` covers admission and durable queueing; if you need delivery confirmation, subscribe to your own domain's bus and observe your events.
- **A retry after an ambiguous outcome is a new admission.** If your client times out after sending the request (you never saw the `202`), a retry may enqueue a second event—the idempotency key supports *your* consumer-side deduplication, and a newly generated `eventId` will differ. UUID metadata is not an exactly-once guarantee.

## Error reference

| Response | Cause | Action |
|----------|-------|--------|
| `400` | Body failed structural validation: a missing field, an unknown field (including an injected top-level `metadata` key), or an invalid event name. On schema writes, also `content` that is not a valid draft-4 document. On version appends, also content that tightens the current latest version; `message` lists the offending paths. | Fix the request body / schema document; for a rejected append, loosen the document or register a new event name instead |
| `403` | No `publisher` binding for this principal on this domain | Ask your platform team for a binding |
| `404` | Domain missing/inactive, or no schema registered for this event in this environment | Check domain status; create the schema first |
| `400` (schema writes) | Called on a non-authoring stage (`AuthoringEnvironmentOnly`) | Create and append on the authoring stage; promote into later environments |
| `409` (create schema) | Schema already exists in this environment | Append a version instead |
| `422` (publish) | `data` violates the schema's latest version—violations are listed in `message` | Fix the payload and retry; if you just appended a schema version, a plain retry picks it up |
| `503` (publish) | The platform could not evaluate the publish (schema registry or queue trouble, or metadata configuration missing/unreadable) | Retry with backoff; alert your platform team if it persists |
| `202` then nothing arrives | Rare: the domain was disabled between admission and delivery, or EventBridge delivery is failing | The platform's publish DLQ holds exhausted messages—contact your platform team |

## Discovering schemas

`GET /schemas?consumerDomain={domain}` is the subscriber catalog: it expands that consumer domain's entitlements into the covered schemas with the latest version per environment—every listed entry is currently subscribable. `GET /schemas/{schemaName}?consumerDomain={domain}&domain={producer}` returns detail including content; uncovered targets answer `403`. Platform roles omit `consumerDomain` for the platform-wide view. See [Domain Management Guide — Discovering schemas](subscriptions.md#discovering-schemas).

## Inspecting composed contracts

`GET /schemas/{schemaName}/contract?domain={yourDomain}&environment={env}` returns the exact composed contract the environment currently serves: the raw business document, the platform-owned metadata schema, generator descriptions, and a composed draft-4 document that validates the full `detail` object offline. Historical inspection is explicit: pass **both** `schemaVersion` and `metadataRevision` (revision `0` is the documented legacy selector for pre-stamping events). The active metadata revision is never silently substituted for a historical business version, and an unknown pair answers `404`. A historical composition is inspection, not evidence that pair was ever emitted—provenance is each event's own recorded tuple. `POST /schemas/contract-preview` composes **unsaved** business content against the environment's active metadata so you can see the effective contract before appending a version; it registers nothing.

In the console, the catalog schema page serves the current pair per environment. Each environment badge is a link that loads that environment's contract into stacked full-width panels. The panels are **Schema content** (the raw business document) and **Schema preview** (the in-force composed document, captioned with its business version, metadata revision, and definition digest). Each panel carries a Copy action that puts the rendered document on the clipboard. Historical pairs are an API capability for clients holding an event's recorded tuple; the console does not expose historical selectors. The producer authoring form's *Preview effective contract* is distinct: it composes unsaved content, is labeled unregistered, and registers nothing.

## Deep dive

For the design rationale, see [Publish validation](../architecture/publish-validation.md). It explains synchronous failures, schema caching, version stamps, and rejected alternatives.

For implementation details, see the architecture sections on [Event publishing](../architecture/publish-validation.md#3-the-two-halves-admission-and-delivery), [Schema management](../architecture/schemas.md#schema-management-append-only), [The canonical event envelope](../reference/event-envelope.md#the-canonical-event-envelope), and [Grant engine](../architecture/authorization.md#grant-engine).
