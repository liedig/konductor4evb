# Proposal

## Why

Konductor changes resources that domain teams depend on (subscriptions, entitlements, schemas, and soon automatic version upgrades through `promote-schema-versions`), but domain teams receive no machine-readable signal when this happens. The roughly twenty existing lifecycle events are internal: they stay on `konductor-bus`, use ad hoc unversioned payloads, and reach only Konductor workflows, owner emails, and the operator SNS topic. Teams want to react automatically, for example with an agent that updates an event handler and opens a pull request when a subscription is upgraded. That needs a stable, versioned contract for platform-to-domain events, defined once as a category rather than separately for each feature.

## What Changes

- Establish **Konductor system events**: a public category of platform-to-domain events, kept separate from both business events and internal control-plane events.
- A **projector** turns committed internal changes into public system events and publishes them to a new central bus, `{org}-konductor-system-bus`, in the platform account. The bus has an EventBridge archive so there is one auditable record. Internal events stay free to change; only the projected shapes are a contract.
- Every system event uses one public source (`konductor.system`) and a shared envelope with the `metadata`/`data` split. `metadata` carries `schemaVersion`, a deterministic `idempotencyKey`, `publishedAt`, a `correlationId` linking events from one operation, and a `resourceRevision` that increases with each change to a resource, so consumers can discard stale or out-of-order events. `data` carries the `audience` (`CONSUMER`, `PRODUCER`, or `OWNER`), the recipient `domain`, the `environment` (null for events that apply to every environment), the affected `resource` (type and id), and event-specific fields.
- **Delivery**: one SNS topic per domain, `{org}-{domain}-system-events`, in the platform account. A single rule on the system bus invokes a fan-out function, which publishes each event to the topic of every domain it concerns. The message body is the full event envelope, and key fields are copied into message attributes for subscription filter policies.
- **Self-service subscription**: each topic's policy allows `sns:Subscribe` only from that domain's registered accounts (`ownerAccountId` plus any `environmentAccounts`), with allowed protocols restricted. Teams subscribe SQS queues, Lambda functions, HTTPS endpoints, or chat integrations from whichever account their tooling runs in. Konductor publishes each domain's topic ARN so teams can find it.
- **Contract governance**: system event schemas are registered in a platform-managed registry and evolve under the same additive-only compatibility gate as business schemas.
- **First event family, subscription upgrades** (driven by `promote-schema-versions`):
  - `SubscriptionUpgradeStarted`: sent to the consumer domain once a re-pin begins (after the promotion's prefix check, or when a first-environment append begins re-pinning).
  - `SubscriptionUpgraded`: sent after narrowing, with outcome `COMPLETED` or `PARTIAL`.
  - Both carry `trigger: PROMOTION | AUTHORING`; the from, intermediate, requested, and resulting versions; the enabled state; the expected re-pin time; and exact contract selectors (business version and metadata revision). They carry no schema documents; consumers fetch contracts through the existing contract API.
- Promotion emails through SES stay as they are for people: a heads-up and a confirmation to consumer domain owners when a promotion re-pins them, and a failure email to producer domain owners. First-environment appends send system events only. There is no notice period.
- Events from before provisioning (registration and provisioning failures) have no topic yet and remain email-only.

## Capabilities

### New Capabilities

- `domain-system-events`: the platform-to-domain event contract. It covers the public source and envelope, audiences, the projector and central system bus with its archive, per-domain topics and their subscription policy, idempotency and ordering guarantees, registration and evolution of system event schemas, and the event families (starting with subscription upgrades).

### Modified Capabilities

- `resource-topology`: a per-domain system-events SNS topic in the platform account, and an explicit allowance for domain accounts to subscribe to it, alongside the existing rule that application teams may not add rules to platform buses.
- `domain-provisioning-workflow`: create each domain's system-events topic and its subscription policy during provisioning, including compensation.
- `event-subscriptions`: emit internal re-pin phase events (started, completed, partial) for each affected subscription, for the projector to consume.
- `domain-owner-notifications`: promotion heads-up and confirmation emails to consumer domain owners, and promotion failure emails to producer domain owners, under the existing conditional notification configuration and SES failure classification.

## Impact

- Depends on `promote-schema-versions` for promotion records, re-pin phases, and producer lifecycle events.
- Infrastructure: the central system bus with its archive; the projector and fan-out functions with DLQs; per-domain SNS topics in the platform account (up to 1,000, well within SNS quotas); a platform registry for system event schemas.
- Lambdas: the projector (internal events to public envelope), the fan-out function (resolves recipient domains, publishes to topics), and new SES notification functions.
- API and console: expose each domain's topic ARN in domain detail and on the domain page.
- Docs: a consumer guide for subscribing to system events (including the agent and pull-request pattern, filter policies, and deduplication), a decision record for the central bus with per-domain topics, and side-effects records for the projector, fan-out, and email paths.
- Assumes no provisioned domains exist at launch, so no topic backfill is needed.
- Open questions for design: exact message attribute set and `sns:Protocol` allowlist; per-domain backfill from the archive (replay fans out to every domain unless filtered); whether to include an optional structural `changes` summary in upgrade events, which needs a diff computed at authoring time because the gate does not list changes; and which existing internal events become the next families (subscription lifecycle, entitlements, schema promotion, metadata activation, domain updates).
