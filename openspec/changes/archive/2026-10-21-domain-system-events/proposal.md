# Proposal

## Why

Schema promotion and authoring appends now automatically upgrade subscriptions, but domain teams have no stable machine-readable notification they can use to update their consumers. Internal lifecycle events are not a public contract; a small, versioned system-event surface will let teams automate reactions without coupling to Konductor's workflow internals.

## What Changes

- Establish **Konductor system events**: a public category of platform-to-domain events, kept separate from both business events and internal control-plane events.
- A **projector** turns committed per-subscription re-pin snapshots into public system events on `{org}-konductor-system-bus` in the platform account. An EventBridge archive retains 90 days for audit. Replay procedures, replay APIs, and historical catch-up delivery are out of this MVP.
- Use source `konductor.system` and an envelope with a `metadata`/`data` split. Metadata identifies the contract version, deterministic idempotency key, publication timestamp, operation correlation, and increasing resource revision. Data identifies one recipient domain, audience, environment, resource, and the event-specific facts. Delivery is at-least-once and unordered; consumers deduplicate and fetch current state when needed.
- Deliver through one platform-owned SNS topic per domain, `{org}-{domain}-system-events`, shared across environments. A routing function publishes the full EventBridge event to the named recipient's topic, with stable SNS filter attributes. The MVP emits only the `CONSUMER` audience.
- Permit self-service SNS subscriptions from the domain's registered accounts (`ownerAccountId` plus effective `environmentAccounts`) for **SQS, Lambda, and email** endpoints; SQS/Lambda endpoint ARNs are restricted to those accounts, while email addresses cannot be endpoint-constrained by topic policy (accepted trade-off: payloads are low-sensitivity version facts). No HTTPS, SMS, or chat subscriptions, and no domain-account permission to publish or administer the topic. Provisioning subscribes the domain owner email by default, and a committed owner-email change reconciles that subscription.
- Register the public event contracts in a platform-managed schema registry. Reuse the existing additive-only compatibility checker; do not expose system schemas through business-schema authoring or promotion.
- Deliver two public event types:
  - `SubscriptionUpgradeStarted` after promotion validation passes and a materialized entry starts re-pinning, or when an authoring append starts re-pinning.
  - `SubscriptionUpgraded` after the entry's outcome is committed, with `COMPLETED` or `PARTIAL`; an individual rule failure must not be reported as a successful upgrade.
  - Include `trigger: PROMOTION | AUTHORING`, original/intermediate/requested/resulting business versions, enabled state, estimated re-pin time, and exact business-version/metadata-revision contract selectors. Exclude schema documents and structural diffs. Existing contract API authorization remains unchanged.
- Add promotion heads-up/outcome emails for consumer owners and promotion-failure emails for producer owners under the existing optional SES configuration. These are new notifications, not existing behavior. Authoring appends send system events only; neither delivery nor email creates a notice period.
- Provision topics for new domains and provide an idempotent operator backfill for existing `ACTIVE` and `INACTIVE` domains without re-provisioning business resources. Expose the recorded topic ARN in domain detail and on a dedicated top-level System events console page (dedicated nav icon) whose unread indicator compares a per-domain latest-event marker against browser-local last-seen state — no event feed or server-side read state.
- Keep registration/provisioning outcomes on their existing internal-event and notification paths. Defer all other public event families, subscription-management UI, and general-purpose event processing infrastructure.

## Capabilities

### New Capabilities

- `domain-system-events`: the public platform-to-domain notification boundary, distinct from business-event delivery and human/operator notifications. Owns the envelope, upgrade event family, delivery semantics, subscriber isolation, audit retention, and schema governance.

### Modified Capabilities

- `resource-topology`: domain-level topics and singleton system resources; an explicit SNS subscription exception to the application-team boundary without platform-bus rule access.
- `domain-provisioning-workflow`: topic creation, policy, tags, compensation, cost-center retagging, non-destructive backfill, the default owner email subscription, and its reconciliation on owner change.
- `event-subscriptions`: durable per-entry upgrade phase snapshots, truthful partial outcomes, and duplicate/superseded operation handling for projection.
- `domain-owner-notifications`: promotion-specific owner emails using existing recipients, conditional configuration, and SES failure classification.
- `domain-management`: additive, authorized discovery of the recorded system-events topic ARN.
- `web-ui`: top-level System events page with topic references, copy feedback, a browser-local unread indicator, and setup guidance with no topic administration controls.

## Impact

- Builds on the implemented and archived `promote-schema-versions` change. Extends `promote-schema` and `repin-schema-env`, the materialization handler, shared stream models, and the event-streams table's durability configuration.
- Adds the system bus/archive/registry, projection and routing functions, retry/failure handling, topic management, and notification wiring to the core stack. Existing bus names and business-event routing do not change.
- Updates both OpenAPI `DomainDetail` shapes, `describe-domain`, and the console domain page. No new public write endpoint is required.
- Adds provisioning/backfill and retag integration for domain-wide SNS resources. Rollout must account for existing domains, a bounded fleet of 1,000 domains, service quotas, and the stream's 24-hour retention.
- Adds a focused consumer setup guide, operator rollout/failure guide, architecture and data-model references, a decision record, side-effects classifications, and an Unreleased changelog entry during implementation.
- Records existing documentation inconsistencies rather than silently treating them as current guarantees, notably platform naming, registry history, and impossible 30-day SQS retention. No project code or main specs are changed by this proposal workflow.
