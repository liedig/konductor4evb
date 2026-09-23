# Schema-first subscriptions: latest-version pinning, resource-policy delivery, event-streams table as source of intent

[Documentation home](../README.md)

- Recorded date: 2026-07-15
- Change ID: `add-event-subscriptions`
- Area: Event Streams
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

**Schema-first**: consumers subscribe to a schema contract—`Subscribe(consumerDomain, producerDomain, schemaName)`—never to a bus/endpoint, and always to the **latest** version, resolved and pinned per environment at subscribe time. One representation of an event at any point in time: subscribers cannot choose historical versions; pinning makes contract transitions observable and enforceable (the version-census substrate for the future evolution thread).

**Canonical envelope**: `source` = env-independent namespace identity (`{org}-{domain}`, platform-derived, never free-typed), schema name = `{source}@{detail-type}` (EventBridge discovery convention, round-trips to `aws schemas describe-schema`), numeric version at `detail.metadata.schemaVersion`; mis-stamped events fail closed (non-delivery, never misdelivery).

**Delivery authorization**: replaced the specced-but-never-built per-subscription bus-to-bus IAM roles with a provision-time resource policy on the consumer's app bus (platform account may `PutEvents`)—attached in `provision-env` (`SetAppBusDeliveryPolicy` step); a backfill for pre-existing domains was built then descoped (unreleased product, no installed base; operator credentials also cannot assume the trust role, which trusts only platform service roles). Subscribe/unsubscribe never touch the consumer account.

**Rule materialization**: one rule per subscription per env on the producer's platform bus, named `{consumerDomain}-{detailType}`—version-agnostic (migration is a pattern update on the same rule), per-bus name uniqueness doubles as duplicate-subscription enforcement, PutRule-as-upsert gives saga idempotency; target DLQ'd to the per-env subscription DLQ via the platform `konductor-subscription-delivery-role`. **Per-env enable/disable** maps to native rule state (subscribe with prod paused; operational kill switch); disabled ≠ removed—contract and pin retained. **`konductor-event-streams` table** (PK `producerDomain`, SK `{schemaName}#{consumerDomain}`, `by-consumer` GSI, TTL) is the source of intent; EventBridge rules are the materialization—topology and version census answered without enumerating AWS resources; REMOVED records retained 90 days. **All-or-nothing activation** across envs with compensation (`activate-subscription.asl.yaml`); idempotent removal (`remove-subscription.asl.yaml`). **Zero-events alarm per subscription** (24 h, missing data = breaching)—a version-pinned rule that stops matching goes silent, not to a DLQ. **Read-only schema discovery** (`GET /schemas`) merges per-env registries reporting latest version per env, making skew visible before subscribing; schema registration/evolution/RBAC explicitly deferred. Subscription id = `{consumer}.{producer}.{detailType}` (domain names cannot contain dots, so parsing is unambiguous).

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-07-21-add-event-subscriptions/proposal.md). The recorded date above is not necessarily the archive or merge date.

## Current relevance

The all-or-nothing subscription saga was superseded by [promotion-following subscriptions](follow-schema-promotion.md). The old guide has not yet been reconciled; see [D01](../reference/documentation-discrepancies.md#d01).

[Decision index](README.md)
