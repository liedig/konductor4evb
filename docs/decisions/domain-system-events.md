# Public system events for subscription upgrades

[Decision index](README.md)

- Recorded date: 2026-10-20.
- Change ID: `domain-system-events`.
- Status: recorded design, extending
  [promote-schema-versions](promote-schema-versions.md) and
  [follow-schema-promotion](follow-schema-promotion.md).

## Decision and rationale

Schema promotion and authoring appends upgrade subscriptions automatically,
and domain teams need a stable, machine-readable notification to react.
Internal lifecycle events stay internal; instead, Konductor defines a small
public system-event surface: source `konductor.system`, a versioned
`metadata`/`data` envelope, and one event family to start
(`SubscriptionUpgradeStarted` / `SubscriptionUpgraded`).

Durability follows the [side-effects rubric](../development/side-effects/README.md):
the re-pin workflow commits self-contained per-entry phase snapshots
(`upgradeEvents`) to the event-streams table inside the same writes that
commit pins or failures, and a DynamoDB-stream projector turns committed
snapshots into public events. Handlers never call SNS, SES, or EventBridge
after committing, so a crash between state change and notification cannot
lose the notification, and public delivery failure cannot undo business
state. Identity is derived from the existing re-pin generation
(`resourceRevision = 2g-1` for start, `2g` for terminal) with a versioned
deterministic idempotency-key hash—no additional counter service.

Delivery routes through one platform-owned SNS topic per domain
(`{org}-{domain}-system-events`, environment-independent), keeping routing
rules constant as domains grow to the fleet cap. Domain accounts get
self-service `sns:Subscribe` for SQS and Lambda endpoints only, restricted
to the domain's registered accounts—topic administration and publication
stay platform-only. Payloads carry exact contract selectors (business
version plus metadata revision) rather than schema documents or diffs, so
consumers fetch contracts through the existing API with its existing
authorization.

The public contracts are checked-in draft-4 schemas with a manifest pinning
version and canonical hash; a deployment-time bootstrap registers them in a
platform system-schema registry behind the existing additive-only
compatibility proof, so contract drift fails deployment instead of silently
shipping. A 90-day EventBridge archive covers audit; replay and historical
catch-up are deliberately out of the MVP. Optional owner emails (consumer
upgrade heads-up/outcome, producer promotion failure) ride the existing
conditional SES configuration and never gate business progress.

## Alternatives considered

- **Publish from the re-pin handler directly**—a failure between the
  DynamoDB commit and the publish would permanently lose the notification.
- **Derive consumer events from aggregate promotion lifecycle events**—they
  lack per-entry original pins, metadata selectors, and truthful per-entry
  outcomes.
- **Per-domain bus rules or a general outbox table**—rejected to keep rule
  counts flat and avoid a second copy of each event; the stream's
  old/new images already preserve successive phases.
- **Replayable public archive**—deferred; current-state APIs plus
  idempotency keys cover consumers, and replay machinery would expand the
  public surface before demand is proven.
