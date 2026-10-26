# System events reference

[Documentation home](../README.md)

Konductor system events are the platform's public, machine-readable notifications to domains. They are separate from business events (which flow on domain buses between producers and consumers) and from internal lifecycle events (which drive the control plane and are not a public contract). Consumers use them to react to platform changes—starting with subscription upgrades—without coupling to workflow internals.

To receive them, subscribe an SQS queue, Lambda function, or email address to your domain's system-events SNS topic; see the [system events guide](../guides/system-events.md). Provisioning subscribes the domain owner email by default (SNS sends a confirmation the owner must accept), and a committed owner-email change moves that default subscription to the new address.

## Delivery model

A projector publishes committed subscription-upgrade phases to a central system event bus, and a router delivers each event to the recipient domain's platform-owned SNS topic `{org}-{domain}-system-events`. One topic serves all of a domain's environments.

- Delivery is **at-least-once**: consumers must deduplicate on `detail.metadata.idempotencyKey`, which is stable across delivery retries.
- Delivery is **unordered** across resources. Within one subscription and environment, `detail.metadata.resourceRevision` increases for successive published upgrade states; gaps are valid, and a lower revision than one already seen is an older state that can be discarded.
- An upgrade **start is not paired with a guaranteed end**: a newer operation can supersede an in-flight one. Track current state by resource revision, and fetch current state via the existing APIs when needed—never count start/end pairs.
- An EventBridge archive retains events for 90 days for audit. There is no replay API; subscribing later does not deliver earlier events.

## Envelope

System events use source `konductor.system` and a `detail` split into platform-owned `metadata` and event `data`:

| Field | Meaning |
| --- | --- |
| `detail.metadata.schemaVersion` | Public event contract version (an integer, starting at 1) |
| `detail.metadata.idempotencyKey` | Deterministic key for one committed phase; stable across retries |
| `detail.metadata.publishedAt` | Logical phase-publication timestamp (ISO-8601 milliseconds) |
| `detail.metadata.correlationId` | The promotion id or authoring execution that owns the phase |
| `detail.metadata.resourceRevision` | Increasing per `(subscription, environment)` upgrade state |
| `detail.data.audience` | `CONSUMER` |
| `detail.data.domain` / `environment` | Recipient domain and the concrete upgrade environment |
| `detail.data.resource` | `{ "type": "SUBSCRIPTION", "id": "<consumer>.<producer>.<detailType>" }` |

The SNS message body is the full EventBridge event. The transport `id` is routing metadata, never identity—parse `detail` instead.

## Three version numbers, three meanings

System events deliberately carry three distinct version concepts:

1. **Event contract version** (`detail.metadata.schemaVersion`)—the version of the *system-event payload itself*, governed by the platform system-schema registry. It changes only when the notification contract changes.
2. **Business schema version** (`fromVersion`, `requestedVersion`, `resultingVersion`)—the producer's schema versions a subscription is pinned to or moving toward.
3. **Metadata revision** (inside `contracts.*.metadataRevision`)—the platform metadata revision composing with a business version. Revision 0 is valid and means legacy (no composed metadata).

## Subscription upgrade events

`SubscriptionUpgradeStarted` is emitted when a committed upgrade begins re-pinning a subscription entry; `SubscriptionUpgraded` is emitted when the outcome commits, with `outcome` `COMPLETED` or `PARTIAL`. A `PARTIAL` outcome carries a safe `reason` (`REPLAY_PARTIAL` or `REPIN_FAILED`) and the actually recorded `resultingVersion`; a failed entry is never reported as delivering normally.

`trigger` distinguishes `PROMOTION` (a schema promotion) from `AUTHORING` (a schema append in the authoring environment). `contracts` carries exact `from`, `requested`, and (in terminal events) `resulting` selectors—producer domain, canonical schema name, environment, business schema version, and metadata revision—that identify the composed contract a consumer can fetch through the existing contract API. `intermediateVersions` is a compact inclusive range of replayed versions strictly between the original and requested versions, or null.

The canonical payload definitions are the checked-in draft-4 schemas
[subscription-upgrade-started.schema.json](https://github.com/liedig/konductor4evb/blob/main/api/system-events/subscription-upgrade-started.schema.json)
and
[subscription-upgraded.schema.json](https://github.com/liedig/konductor4evb/blob/main/api/system-events/subscription-upgraded.schema.json),
registered in the platform's system-schema registry with additive-only compatibility enforced at deployment.

## SNS filter attributes

Every publication carries message attributes so subscribers can filter without parsing the body: `eventType`, `schemaVersion` (Number), `audience`, `domain`, `environment`, `resourceType`, and `trigger`; terminal events add `outcome`. Attribute values always agree with the body. A null environment maps to the attribute value `ALL`.

## Optional owner emails

When the platform enables SES notifications (`NotificationsEnabled`), the same events also produce human-readable email, fully independent of machine delivery:

- **Consumer domains** (`SubscriptionUpgradeStarted`, `SubscriptionUpgraded`): the domain owner and secondary owners receive an email **only when `data.trigger` is `PROMOTION`**. Authoring re-pins stay silent. A `PARTIAL` outcome says so plainly and names the reached version and reason.
- **Producer domains** (`SchemaPromotionFailed`): owners receive an email when their promotion fails, with the failure classification (for example `DIVERGED`, meaning consumer environments must converge before re-pinning) and the partial `failedVersion` when the replay produced one.

Email limits: messages can be delayed, duplicated, or arrive out of order relative to the SNS stream, and there is no replay. Email is advisory; the SNS topic and the version APIs are the sources of truth. Transient SES failures retry at the rule and land in the system-event DLQ; permanent failures (for example an unverified sender address) are acknowledged without retry so they never block event delivery. Disabling notifications removes the email rules and sender function only — topics, routing, and the archive stay deployed.
