# Capability: Event Subscriptions

## Purpose

How consumer domains connect to producer event contracts: schema-first subscriptions that pin the latest schema version per environment, materialize as one EventBridge rule per environment with presence-scoped activation that follows schema promotion into new environments automatically, and are governed by subscriber bindings and entitlement coverage rather than account ownership.

## Requirements

### Requirement: Schema-first subscription

Consumers SHALL subscribe a registered domain to another registered domain's event schema by specifying `consumerDomain`, `producerDomain`, and `schemaName`. The subscription SHALL bind to the **latest** version of the schema in each environment where the schema exists, resolved and pinned at materialization time; environments where the schema does not exist are recorded as awaiting promotion rather than failing the subscription (see Presence-scoped per-environment materialization). The API SHALL NOT allow subscribing to a historical schema version, and SHALL NOT accept a caller-supplied event pattern — the schema name alone determines the filter.

#### Scenario: Subscribe to a schema

- **WHEN** an authenticated caller submits `POST /subscriptions` with `consumerDomain: checkout`, `producerDomain: orders`, and `schemaName: prezzee-orders@OrderNotification`
- **THEN** the control plane SHALL resolve the latest version of `prezzee-orders@OrderNotification` in each of the producer's per-environment schema registries where the schema exists
- **AND THEN** pin the resolved version per materialized environment in the subscription's event stream rules and in the stream record

#### Scenario: Historical version cannot be subscribed

- **WHEN** a caller attempts to specify a schema version in a subscribe request
- **THEN** the request SHALL be rejected by structural validation — the subscribe operation accepts no version parameter

#### Scenario: Per-environment version skew is recorded faithfully

- **WHEN** the latest version of the schema differs across environments (e.g., `5` in dev, `3` in prod)
- **THEN** each environment's rule SHALL pin that environment's latest version
- **AND THEN** the stream record SHALL show the pinned version per environment

### Requirement: Event stream materialization

Each subscription SHALL materialize as exactly one EventBridge rule per materialized environment on the producer's platform-side domain event bus; environments awaiting promotion carry no rule until the schema arrives. The rule SHALL be named `{consumerDomain}-{detailType}` — the version SHALL never appear in the rule name. The rule pattern SHALL filter on `source` (the producer's namespace identity), `detail-type` (the event name), and the numeric `detail.metadata.schemaVersion`. Outside a re-pin, the version filter SHALL match the version pinned for that environment and the version immediately after it. The next version is matched so that an authoring append is delivered from its first event, before re-pinning catches up. During a re-pin, the filter SHALL match every version from the pin through the re-pin's target, plus the version after the target. The rule target SHALL be the consumer's app-side domain event bus for the same environment, with a `DeadLetterConfig` referencing the producer's per-environment subscription DLQ and a retry policy.

#### Scenario: Rule created per environment

- **WHEN** a subscription from `checkout` to `prezzee-orders@OrderNotification` (pinned at version 3 in prod) is materialized in prod
- **THEN** the prod platform bus `{org}-prod-orders-event-bus` SHALL carry a rule named `checkout-OrderNotification` with pattern `{"source": ["prezzee-orders"], "detail-type": ["OrderNotification"], "detail": {"metadata": {"schemaVersion": [3, 4]}}}`
- **AND THEN** the rule SHALL target `{org}-prod-checkout-event-bus` in the consumer's domain account
- **AND THEN** the rule target SHALL configure `DeadLetterConfig` pointing at `{org}-prod-orders-subscription-dlq`

#### Scenario: Version migration is a pattern update, not rule replacement

- **WHEN** a subscription's pinned version changes through a re-pin
- **THEN** the change SHALL be applied as a `PutRule` pattern update against the same rule name
- **AND THEN** no rule deletion or re-creation SHALL occur, preserving delivery continuity, metrics, and DLQ configuration

#### Scenario: An authoring append is delivered before re-pinning

- **WHEN** a dev subscription is pinned at v3 and a publisher appends v4 in dev
- **THEN** events stamped v4 SHALL be delivered from the first one, before the re-pin moves the pin to v4

#### Scenario: No consumer-account mutation at subscribe time

- **WHEN** a subscription is created or removed
- **THEN** the control plane SHALL NOT assume the trust role or modify any resource in the consumer's domain account — delivery is authorized by the provision-time resource policy on the consumer's app-side bus
### Requirement: Presence-scoped per-environment materialization

Subscription activation SHALL evaluate each producer environment independently. An environment whose registry contains the schema SHALL materialize (rule, target, zero-events alarm) with that environment's latest version pinned. An environment whose registry does not contain the schema SHALL be recorded as `AWAITING_PROMOTION` and SHALL NOT fail the subscription. One environment's materialization failure SHALL NOT remove, disable, or otherwise affect another environment's rule; there SHALL be no cross-environment compensation. The subscription's overall status SHALL become `ACTIVE` when at least one environment materializes. The subscription SHALL become `FAILED` with an error detail identifying the schema and producer when no environment materializes and none is merely awaiting (the schema exists nowhere — almost always a caller error).

#### Scenario: Schema present in a subset of environments

- **WHEN** the schema exists in the dev registry but not in staging or prod, and a covered consumer subscribes
- **THEN** the dev environment SHALL materialize with dev's latest version pinned and the rule `ENABLED`
- **AND THEN** staging and prod SHALL be recorded as `AWAITING_PROMOTION` with no rule created
- **AND THEN** the stream record status SHALL be `ACTIVE`

#### Scenario: Schema absent from every environment fails fast

- **WHEN** a caller subscribes to a schema name that exists in none of the producer's environment registries
- **THEN** the stream record SHALL become `FAILED` with an error detail stating the schema was not found in any environment of the producer
- **AND THEN** a `SubscriptionActivationFailed` lifecycle event SHALL be emitted

#### Scenario: One environment's failure is contained

- **WHEN** the schema exists in dev and staging, dev materializes, and staging rule creation fails transiently
- **THEN** the dev rule SHALL remain live and untouched
- **AND THEN** the staging entry SHALL be recorded `FAILED` with the failure cause on that environment's entry
- **AND THEN** the stream record status SHALL be `ACTIVE`

### Requirement: Promotion-following environment extension

When a `SchemaVersionPublished` event announces a schema's arrival in an environment, the control plane SHALL extend every `ACTIVE` subscription on that schema whose record lacks a materialized entry for that environment: the environment SHALL materialize as in initial activation (latest version resolved and pinned, rule, target, alarm), with a final covering-entitlement check before the rule goes `ENABLED`. Because a promotion emits this event once, after its last registry write, the resolved latest version SHALL be the promotion's final version, never an intermediate replayed version. The new rule SHALL be `ENABLED` unless a desired disabled state was recorded for that environment. Extension SHALL be idempotent: an environment already materialized is a no-op for extension, and version changes in materialized environments SHALL follow the re-pinning requirements instead. Streams in `PENDING`, `FAILED`, or `REMOVED` status SHALL NOT be extended. A successful extension SHALL emit a `SubscriptionEnvironmentExtended` lifecycle event with the subscription identity, environment, pinned version, and enabled state.

#### Scenario: Promotion extends an existing subscription

- **WHEN** a subscription is `ACTIVE` with only dev materialized and the producer promotes the schema into staging
- **THEN** the subscription SHALL materialize staging with staging's latest version pinned and the rule `ENABLED`
- **AND THEN** a `SubscriptionEnvironmentExtended` event SHALL be emitted
- **AND THEN** the dev entry SHALL be unchanged

#### Scenario: Extension pins the promotion's final version

- **WHEN** a schema first arrives in staging through a promotion that replays v1, v2, and v3
- **THEN** an awaiting staging entry SHALL materialize pinned at v3

#### Scenario: Pre-declared pause is honored at promotion

- **WHEN** a subscription recorded prod as desired-disabled while prod was awaiting promotion, and the schema later arrives in prod
- **THEN** the prod rule SHALL be created with state `DISABLED`
- **AND THEN** no events SHALL be delivered to the consumer's prod bus until prod is enabled

#### Scenario: Revoked entitlement blocks extension

- **WHEN** the covering entitlement was revoked before a promotion event triggers extension
- **THEN** the final entitlement check SHALL fail closed, no rule SHALL be created or enabled, and the environment entry SHALL record the denial

#### Scenario: Append in an existing environment is a no-op

- **WHEN** a `SchemaVersionPublished` event announces a new version in an environment the subscription already has materialized
- **THEN** extension SHALL make no change to that entry
- **AND THEN** any change to its pinned version SHALL follow the re-pinning requirements
### Requirement: Subscription identity and idempotency

A subscription SHALL be identified by the triple (consumerDomain, producerDomain, detailType), exposed in the API as `{consumerDomain}.{producerDomain}.{detailType}`. Domain names cannot contain dots, so the identifier SHALL be parsed by splitting on the first two dots; the event name portion may itself contain dots. There SHALL be at most one subscription per triple. A subscribe request for an existing active subscription SHALL be idempotent. On a given producer bus, the rule name `{consumerDomain}-{detailType}` is bijective with the subscription identity — EventBridge's per-bus rule-name uniqueness SHALL serve as duplicate-subscription enforcement, and rule creation SHALL be an upsert (`PutRule`) so retried workflow steps are safe.

#### Scenario: Duplicate subscribe is idempotent

- **WHEN** a caller subscribes and an `ACTIVE` subscription already exists for the same (consumer, producer, detailType)
- **THEN** the operation SHALL succeed without creating duplicate resources
- **AND THEN** the outcome SHALL reference the existing subscription

#### Scenario: Re-subscribe after removal creates a fresh contract

- **WHEN** a caller subscribes and a `REMOVED` record exists for the same triple
- **THEN** a new subscription SHALL be created, re-resolving the latest schema versions at this point in time

### Requirement: Event streams registry

All event streams SHALL be registered in a dedicated `konductor-event-streams` DynamoDB table — the source of intent, of which EventBridge rules are the materialization. Topology questions SHALL be answerable from the table without enumerating AWS resources. The table SHALL use partition key `producerDomain` and sort key `{schemaName}#{consumerDomain}`, with a GSI keyed by `consumerDomain`. Each record SHALL store the subscription metadata, overall status, and a per-environment map of `{pinnedVersion, ruleName, ruleArn, enabled, status, errorDetail?}`. Every producer environment SHALL appear in the map: materialized environments carry their pinned version, rule ARN, and `ACTIVE` (or per-environment `FAILED` with the cause in that entry's `errorDetail`); environments where the schema has not landed SHALL carry status `AWAITING_PROMOTION` with no rule ARN or pinned version.

#### Scenario: Version census for a schema

- **WHEN** a governance process asks which consumers are subscribed to `prezzee-orders@OrderNotification` and on which versions
- **THEN** a single query on `producerDomain = orders` with sort-key prefix `prezzee-orders@OrderNotification#` SHALL return every subscriber with per-environment pinned versions
- **AND THEN** no EventBridge API calls SHALL be required

#### Scenario: Consumer's inbound streams

- **WHEN** a team asks which event streams feed the `checkout` domain
- **THEN** a query on the consumer GSI SHALL return all subscriptions where `checkout` is the consumer, with producer, schema, per-environment versions, and status

#### Scenario: Disabled subscriptions remain in the census

- **WHEN** a subscription is disabled in one or more environments
- **THEN** it SHALL still appear in version-census and topology queries, distinguishable by its per-environment `enabled` flags

#### Scenario: Awaiting environments are visible in topology

- **WHEN** a subscription has environments awaiting promotion
- **THEN** census and topology queries SHALL show those environments with status `AWAITING_PROMOTION` and no pinned version

### Requirement: Per-environment enable and disable

A subscription SHALL carry per-environment desired state (`enabled: true|false`). For materialized environments the desired state maps to the native EventBridge rule state (`ENABLED|DISABLED`); for environments awaiting promotion the desired state is recorded and honored when promotion materializes the environment. Initial per-environment state SHALL be settable in the subscribe request, defaulting to enabled in every environment, and SHALL be accepted for any producer environment — including ones where the schema has not landed yet. State SHALL be updatable via `POST /subscriptions/state`. Disabling SHALL NOT remove the rule, unpin the version, or void the contract. Setting `enabled: true` on an environment whose materialization `FAILED` SHALL re-trigger materialization for that environment — this is the repair path for transient per-environment failures.

#### Scenario: Subscribe with production paused

- **WHEN** a caller subscribes with `environments: {prod: {enabled: false}}`
- **THEN** the prod rule SHALL be created with state `DISABLED` while dev and staging rules are `ENABLED`
- **AND THEN** no events SHALL be delivered to the consumer's prod bus until prod is enabled

#### Scenario: Pause before the schema reaches an environment

- **WHEN** a caller subscribes with `environments: {prod: {enabled: false}}` and the schema does not yet exist in prod
- **THEN** the prod entry SHALL be recorded `AWAITING_PROMOTION` with desired state disabled
- **AND THEN** when the schema is later promoted to prod, the rule SHALL be created `DISABLED`

#### Scenario: Disable as an operational kill switch

- **WHEN** a subscription is flooding the consumer or must be halted during an incident
- **THEN** a `POST /subscriptions/state` setting `enabled: false` for the affected environments SHALL stop delivery via an atomic rule-state change
- **AND THEN** re-enabling SHALL restore delivery without re-subscribing or losing the pinned version

#### Scenario: Re-enabling a failed environment retries materialization

- **WHEN** an environment's entry is `FAILED` (for example after a transient rule-creation error) and the caller sets `enabled: true` for it
- **THEN** materialization SHALL be re-attempted for that environment
- **AND THEN** on success the entry SHALL become `ACTIVE` with the pinned version and rule ARN recorded

### Requirement: Unsubscribe

Unsubscribing SHALL remove the subscription's rules (targets, then rule) from the producer's platform bus in every environment, idempotently treating already-deleted resources as no-ops. The stream record SHALL be marked `REMOVED` with a `removedAt` timestamp and retained for 90 days via DynamoDB TTL for audit. The control plane SHALL NOT touch the consumer's app-side bus: integration rules the application team created there are their responsibility, including cleanup.

#### Scenario: Clean unsubscribe

- **WHEN** the subscription owner sends `DELETE /subscriptions/{subscriptionId}`
- **THEN** all per-environment rules for the subscription SHALL be removed from the producer's platform buses
- **AND THEN** event delivery to the consumer SHALL stop
- **AND THEN** the stream record SHALL have status `REMOVED` and an expiry 90 days after removal

#### Scenario: Orphaned consumer rules are the application team's responsibility

- **WHEN** a subscription is removed and the consumer's app-side bus still carries integration rules for the removed stream
- **THEN** the control plane SHALL NOT modify or delete those rules
- **AND THEN** the application team SHALL manage their own bus's rule hygiene

### Requirement: Subscription validation and grant authorization

Before activating a subscription, the control plane SHALL validate that: both the consumer and producer domains exist and are `ACTIVE`; the derived rule name satisfies EventBridge constraints (charset `[A-Za-z0-9._-]`, length ≤ 64 characters); and the grant engine allows the subscribe action for the normalized trusted principal against the composite resource `{consumerDomain}/{producerDomain}/{eventName}`. Authorization SHALL NOT use account-ownership matching against the consumer domain's `ownerAccountId`. Create and enable SHALL require a `subscriber` binding on the consumer domain plus a current covering entitlement; disable and delete SHALL require only the binding. Self-subscription (consumer equals producer) SHALL be permitted and implicitly covered — it is how a domain consumes its own events on its app-side bus. `platform-admin` MAY operate subscriptions without a subscriber binding while still requiring current covering entitlement for create and enable. Unauthorized attempts SHALL fail with `403 Forbidden` and a deterministic `*Failed` outcome event; deny-as-unknown concealment SHALL NOT be applied.

#### Scenario: Covered subscriber creates a subscription

- **WHEN** a caller holds a `subscriber` binding on the consumer domain and the consumer domain holds a current covering entitlement for the target producer schema
- **THEN** the subscribe command SHALL pass authorization and proceed to validation and activation

#### Scenario: Missing binding or entitlement denied

- **WHEN** a caller lacks a `subscriber` binding on the consumer domain, or no current entitlement covers the target producer schema
- **THEN** the command SHALL fail with `403 Forbidden`
- **AND THEN** a `SubscribeFailed` outcome event SHALL carry the authorization failure without tokens, claims, or payload detail

#### Scenario: Account ownership no longer authorizes

- **WHEN** a caller whose AWS account matches the consumer domain's `ownerAccountId` but who holds no `subscriber` binding attempts to subscribe
- **THEN** the command SHALL fail authorization with `403 Forbidden`

#### Scenario: Post-revocation lifecycle control retained

- **WHEN** the entitlement covering an existing subscription has been revoked
- **THEN** an authorized subscriber SHALL remain able to disable or delete the subscription
- **AND THEN** create or enable SHALL be denied

#### Scenario: Inactive domain rejected

- **WHEN** either domain in a subscribe request is not in `ACTIVE` status
- **THEN** the command SHALL fail validation identifying the offending domain and its current status

#### Scenario: Rule name budget exceeded

- **WHEN** `{consumerDomain}-{detailType}` exceeds 64 characters or contains characters outside `[A-Za-z0-9._-]`
- **THEN** the subscribe command SHALL fail validation with an error explaining the constraint

#### Scenario: Self-subscription permitted

- **WHEN** a caller with a `subscriber` binding on a domain subscribes that domain to one of its own schemas
- **THEN** the subscription SHALL be created normally without any entitlement, delivering the domain's events to its own app-side bus

### Requirement: Final entitlement check before enablement

Immediately before any workflow transitions a Konductor-managed EventBridge rule to `ENABLED` — including initial activation, per-environment enable operations, and promotion-following extension — the control plane SHALL perform a final current-entitlement check through the grant engine for the subscription's consumer domain and producer schema. If no current entitlement covers the schema at that moment, the workflow SHALL NOT enable the rule and SHALL record the denial.

#### Scenario: Revocation during activation blocks enablement

- **WHEN** the covering entitlement is revoked after a subscribe command was accepted but before its activation workflow enables a rule
- **THEN** the workflow SHALL NOT transition any rule to `ENABLED`
- **AND THEN** the workflow SHALL record the entitlement denial as the outcome

#### Scenario: Revocation before promotion extension blocks enablement

- **WHEN** the covering entitlement is revoked after a promotion event is emitted but before the extension enables the new environment's rule
- **THEN** the extension SHALL NOT create or enable the rule
- **AND THEN** the environment entry SHALL record the entitlement denial

#### Scenario: Enablement proceeds while covered

- **WHEN** a current covering entitlement exists at the moment of the final check
- **THEN** the workflow SHALL enable the rule as designed

### Requirement: Subscription read operations

The control plane SHALL provide `GET /subscriptions?domain={domain}&direction=inbound|outbound` and canonical `GET /subscriptions/{subscriptionId}`. The `direction` parameter SHALL be required; the global list route SHALL NOT exist. Inbound SHALL list subscriptions where the queried domain consumes, authorized by a `subscriber` binding on that domain. Outbound SHALL list subscriptions consuming the queried producer domain's schemas, authorized by a `publisher` binding on that domain, and SHALL return full consumer topology because producer teams need it for notifications and future version migration. Both SHALL support schema, pinned-version, and status filters and SHALL be served synchronously from the event-streams store. Canonical detail SHALL authorize against the visible consumer side, the visible producer side, or a platform role (`platform-admin`, `platform-auditor`), returning full stream detail including per-environment pinned versions, rule ARNs, enabled flags, and materialization status. List items and detail SHALL include the recorded `errorDetail` (top-level and per-environment) whenever present, so a failed subscription's reason is visible without table access. Unauthorized detail or list requests SHALL return `403 Forbidden`; a missing subscription SHALL return `404 Not Found`.

#### Scenario: Subscriber lists inbound subscriptions

- **WHEN** a caller with a `subscriber` binding on `checkout` requests `GET /subscriptions?domain=checkout&direction=inbound`
- **THEN** the response SHALL list `checkout`'s subscriptions with the requested filters applied

#### Scenario: Publisher lists outbound consumers

- **WHEN** a caller with a `publisher` binding on `orders` requests `GET /subscriptions?domain=orders&direction=outbound`
- **THEN** the response SHALL list all subscriptions consuming `orders` schemas, including consumer domains and pinned versions

#### Scenario: Failed subscription exposes its reason

- **WHEN** a subscription is `FAILED` (or has per-environment failures) and an authorized caller requests its detail
- **THEN** the response SHALL include the recorded failure cause
- **AND THEN** list responses SHALL carry the same detail for failed items

#### Scenario: List subscriptions by producer and version

- **WHEN** a caller with a `publisher` binding on `orders` requests `GET /subscriptions?domain=orders&direction=outbound&schema=prezzee-orders@OrderNotification&version=3`
- **THEN** the response SHALL list subscriptions pinned to version 3 of that schema in at least one environment, indicating which environments

#### Scenario: Direction is required

- **WHEN** a caller requests `GET /subscriptions?domain=checkout` without `direction`
- **THEN** the API SHALL reject the request identifying the required parameter

#### Scenario: Describe a subscription

- **WHEN** a caller authorized through the consumer side, the producer side, or a platform role requests `GET /subscriptions/checkout.orders.OrderNotification`
- **THEN** the response SHALL include the full per-environment map (pinned version, rule ARN, enabled, status), overall status, and timestamps

#### Scenario: Unauthorized detail returns 403

- **WHEN** a caller with no authorization on either side requests an existing subscription's detail
- **THEN** the API SHALL return `403 Forbidden`
- **AND THEN** a request for a subscription that does not exist SHALL return `404 Not Found`

### Requirement: Subscription lifecycle status and events

A subscription SHALL progress through `PENDING` (command accepted, activation running) → `ACTIVE` (at least one environment materialized) | `FAILED` (no environment materialized, or validation failure) → `REMOVED` (unsubscribed, in audit retention). Individual environments carry their own status (`PENDING`, `ACTIVE`, `AWAITING_PROMOTION`, `FAILED`), which may diverge from one another and from the overall status. The activation and removal workflows SHALL emit lifecycle events on the control-plane bus: `SubscriptionActivated`, `SubscriptionActivationFailed`, and `SubscriptionRemoved`, each carrying the subscription identity and per-environment outcomes. Promotion-following extension SHALL emit `SubscriptionEnvironmentExtended` carrying the subscription identity, the environment, the pinned version, and the enabled state.

#### Scenario: Lifecycle events emitted

- **WHEN** activation completes with at least one environment materialized
- **THEN** a `SubscriptionActivated` event SHALL be emitted with the subscription identity and per-environment outcomes (materialized versions, awaiting environments, per-environment failures)
- **AND WHEN** no environment materializes
- **THEN** a `SubscriptionActivationFailed` event SHALL be emitted carrying the error details

#### Scenario: Extension emits its own lifecycle event

- **WHEN** a promotion-following extension materializes a new environment on an `ACTIVE` subscription
- **THEN** a `SubscriptionEnvironmentExtended` event SHALL be emitted with the subscription identity, environment, pinned version, and enabled state

### Requirement: Subscriptions follow the latest version

When a promotion finishes writing into an environment, or an append commits in the authoring environment, Konductor SHALL re-pin every materialized entry for that schema in that environment to the environment's latest version, whether the entry is enabled or disabled. Entries that are `AWAITING_PROMOTION` or `FAILED` SHALL NOT be re-pinned; extension and the re-enable repair path already resolve the latest version.

#### Scenario: Promotion re-pins staging subscribers

- **WHEN** `checkout` and `billing` subscriptions are materialized in staging pinned at v1, and a promotion brings staging to v3
- **THEN** both staging entries SHALL end pinned at v3
- **AND THEN** their dev and prod entries SHALL be unchanged

#### Scenario: Authoring append re-pins dev subscribers

- **WHEN** a publisher appends v4 in the authoring environment `dev` where a subscription is pinned at v3
- **THEN** the dev entry SHALL end pinned at v4

#### Scenario: Paused subscriptions are re-pinned too

- **WHEN** a materialized staging entry is disabled and a promotion brings staging to v3
- **THEN** the entry SHALL end pinned at v3 and remain disabled

### Requirement: Delivery continues while a pin moves

A re-pin SHALL NOT drop events stamped with any version between the old pin and the new one. Before a promotion writes versions, each affected rule SHALL be widened to match every version from its current pin through the requested version. The rule SHALL be narrowed only after a drain window at least as long as the publish queue's retry horizon. The entry's recorded `pinnedVersion` SHALL change when narrowing commits.

#### Scenario: Events stamped mid-promotion are delivered

- **WHEN** a staging subscription is pinned at v1 and a promotion to v3 is in progress
- **AND WHEN** events stamped v1, v2, and v3 are published to staging during the promotion
- **THEN** all of them SHALL be delivered to the consumer's staging bus

#### Scenario: Narrowing waits for the drain window

- **WHEN** a promotion has written its last version
- **THEN** affected rules SHALL keep matching the earlier versions until the drain window has elapsed
- **AND THEN** after narrowing, the entry SHALL record the new `pinnedVersion`

#### Scenario: Pinned version is stable until narrowing

- **WHEN** a reader queries a subscription whose staging entry is being re-pinned from v1 to v3
- **THEN** the entry SHALL report `pinnedVersion` 1 until narrowing commits, and 3 afterwards

### Requirement: Partial replay pins to the last written version

When a promotion fails after writing some versions, affected rules SHALL narrow to the last version actually written, after the same drain window, rather than to the requested version or back to the old pin.

#### Scenario: Replay stops at v2

- **WHEN** a promotion into staging from v1 to v3 writes v2 and then fails
- **THEN** affected staging rules SHALL narrow to v2 after the drain window
- **AND THEN** the entries SHALL record `pinnedVersion` 2

#### Scenario: Nothing written leaves pins unchanged

- **WHEN** a promotion fails before writing any version
- **THEN** affected rules SHALL return to their pattern from before the promotion, and recorded pins SHALL be unchanged

### Requirement: Re-pinning preserves subscription state

Re-pinning SHALL update the existing rule's pattern under the same rule name, SHALL preserve each entry's enabled or disabled state, SHALL NOT change a rule's state to `ENABLED`, and SHALL NOT modify any resource in the consumer's account.

#### Scenario: Same rule, new pattern

- **WHEN** a subscription's staging pin moves from v1 to v3
- **THEN** the change SHALL be a pattern update on rule `checkout-OrderNotification`
- **AND THEN** its target, dead-letter configuration, metrics, and alarm SHALL be unchanged

#### Scenario: Re-pinning never enables a rule

- **WHEN** a re-pin runs on an entry whose rule is `DISABLED`
- **THEN** the rule SHALL remain `DISABLED`

### Requirement: Overlapping re-pins converge on the latest version

When a new re-pin starts for an environment while an earlier re-pin of the same schema is still draining there, the earlier one SHALL NOT narrow a rule so that it excludes versions the newer one widened to. Every affected entry SHALL end pinned at the environment's latest version.

#### Scenario: Two appends in quick succession

- **WHEN** a publisher appends v4 and then v5 in dev before the v4 re-pin has narrowed
- **THEN** no event stamped v3, v4, or v5 SHALL be dropped by the re-pins
- **AND THEN** the dev entry SHALL end pinned at v5

### Requirement: Re-pin failures are contained per entry

A failure to re-pin one subscription entry SHALL NOT stop re-pinning of other entries, SHALL NOT fail the promotion, and SHALL be recorded on that entry as `FAILED` with an `errorDetail`. Re-enabling the entry SHALL repair it by re-materializing at the environment's latest version.

#### Scenario: One subscriber fails to re-pin

- **WHEN** a promotion re-pins four staging subscriptions and the pattern update for one fails
- **THEN** the other three SHALL be re-pinned
- **AND THEN** the failing entry SHALL be recorded `FAILED` with the cause
- **AND THEN** re-enabling it SHALL materialize it at staging's latest version

### Requirement: Durable subscription upgrade phase facts

Each materialized entry entering re-pinning SHALL commit self-contained start facts identifying its operation, trigger, original pin, requested version, environment, consumer, enabled state, contract selectors, and estimated completion. Terminal facts SHALL be committed atomically with the resulting pin or recorded entry failure. Public notification delivery SHALL derive from committed facts, not reconstruct history from mutable current state.

#### Scenario: Per-entry starting pins differ
- **WHEN** two subscriptions on one schema start a promotion with different recorded pins
- **THEN** each start snapshot SHALL retain that entry's original pin rather than the schema-wide minimum

#### Scenario: Notification publication fails after pin commit
- **WHEN** an entry's new pin and outcome commit but public event publication fails
- **THEN** the same committed outcome SHALL remain available to the retry path
- **AND** publication SHALL NOT rerun business-schema replay or invent a new operation identity

#### Scenario: Entry failure is committed with its outcome
- **WHEN** a narrowing operation cannot update an entry's rule
- **THEN** the recorded failure and partial-outcome facts SHALL be committed together
- **AND** failure to persist those facts SHALL propagate to the durable workflow retry/failure path rather than be silently swallowed

### Requirement: Upgrade notifications preserve re-pin behavior

Notifications SHALL preserve existing widening, drain, narrowing, enablement, and per-entry failure isolation. A successful terminal event SHALL follow the committed entry outcome, not registry publication or schema-wide narrowing intent. Entries awaiting promotion or already failed SHALL NOT generate start notifications merely because they appear in the schema's subscription inventory.

#### Scenario: Registry is ready before subscriptions finish draining
- **WHEN** the schema version has been published but an entry is still draining
- **THEN** no completed upgrade event SHALL be emitted for that entry yet

#### Scenario: Missing environment is extended instead of re-pinned
- **WHEN** a promotion materializes an environment that was awaiting promotion
- **THEN** the existing extension behavior SHALL apply
- **AND** the MVP SHALL NOT mislabel that first materialization as an upgrade from a fictitious prior version

#### Scenario: Notification delivery does not gate business progress
- **WHEN** SNS delivery or SES email is delayed or fails
- **THEN** the committed upgrade SHALL not be reversed or held waiting for acknowledgement
- **AND** publication failures SHALL remain observable independently

### Requirement: Duplicate and superseded upgrade phases

Retrying a committed phase SHALL reuse its identity and SHALL NOT advance its resource revision again. A newer re-pin SHALL supersede an older operation's uncommitted outcome. Superseded work SHALL NOT publish a misleading terminal event or overwrite newer recorded upgrade facts. A start event SHALL NOT promise a one-to-one terminal event if a later operation supersedes it.

#### Scenario: Materialization task is delivered twice
- **WHEN** a task retries after its phase facts were already committed
- **THEN** the persisted facts SHALL remain unchanged
- **AND** any repeated notification SHALL carry the original phase's idempotency key

#### Scenario: New append supersedes a draining append
- **WHEN** a v5 authoring re-pin supersedes a draining v4 re-pin
- **THEN** the v4 operation SHALL not emit a new completion claiming the entry is now at v4 after v5 has taken over
- **AND** the surviving v5 outcome SHALL describe the actual resulting pin with a higher resource revision
- **AND** consumers SHALL be able to reconcile superseded starts using the current subscription API

### Requirement: Contract selectors are snapshotted with each phase

Upgrade facts SHALL snapshot the exact metadata revision used to describe each phase's business contracts, rather than resolve the current metadata revision during projection. Metadata revision 0 SHALL remain a valid legacy selector. The event contract version SHALL be independent of both business schema versions and metadata revisions.

#### Scenario: Metadata changes during the drain
- **WHEN** metadata activation changes between an entry's start and terminal phases
- **THEN** the start facts SHALL retain their original metadata revision
- **AND** the terminal facts SHALL identify the metadata revision observed for that outcome
- **AND** replaying either fact through publication SHALL not replace its selector with the latest revision

### Requirement: Python SDK subscription commands

The Python SDK SHALL expose `subscribe` with consumer domain, producer domain, schema name, and optional per-environment desired state, and `unsubscribe` with subscription identifier. Both SHALL use the existing commands and return acceptance receipts, not completed subscription resources. Neither operation SHALL receive events or provision application-side integrations.

#### Scenario: Subscribe with one environment paused
- **WHEN** a caller subscribes with production disabled and other environments omitted
- **THEN** the SDK SHALL preserve the explicit disabled value and omit unprovided environments
- **AND** it SHALL leave environment defaults, version selection, authorization, and activation to the service

#### Scenario: Accepted command can later fail
- **WHEN** subscribe or unsubscribe returns HTTP 202 with a nonempty `requestId`
- **THEN** the SDK SHALL return the request identifier as an acceptance receipt
- **AND** it SHALL NOT assert that resource authorization passed or activation or removal completed

#### Scenario: Unsubscribe preserves dotted event names
- **WHEN** a caller unsubscribes using `checkout.orders.order.placed.v2`
- **THEN** the SDK SHALL address the existing subscription-detail path with that complete identifier
- **AND** it SHALL NOT truncate the event name, send a replacement request body, or infer a different subscription

### Requirement: Python SDK subscription detail inspection

The Python SDK SHALL expose `describe_subscription` as a synchronous read of the existing detail route. It SHALL preserve the returned identity, timestamps, overall status, failure detail, and per-environment pinned versions, rule information, desired state, and materialization status. Absent or nullable fields SHALL NOT cause valid responses to fail decoding.

#### Scenario: Partial activation is visible
- **WHEN** detail reports overall `ACTIVE`, a materialized dev environment, and prod `AWAITING_PROMOTION` without a pin or rule ARN
- **THEN** the SDK SHALL preserve those distinct states and missing values
- **AND** it SHALL NOT describe all environments as active

#### Scenario: Failures and removal are visible
- **WHEN** detail contains top-level or per-environment `errorDetail`, or a `removedAt` value
- **THEN** the SDK SHALL expose the supplied information
- **AND WHEN** the top-level nullable fields are null or optional environment fields are omitted
- **THEN** the SDK SHALL decode the response without inventing values

#### Scenario: Additive response evolution
- **WHEN** the service adds response fields or returns an unfamiliar status string
- **THEN** the SDK SHALL tolerate additional fields and preserve the status value without treating it as successful activation

#### Scenario: Detail is unavailable or forbidden
- **WHEN** detail returns HTTP 404 or 403
- **THEN** the SDK SHALL raise the corresponding service error
- **AND** it SHALL NOT convert the response into a removed subscription or successful unsubscribe

### Requirement: SDK inspection is not command completion tracking

The Python SDK SHALL document subscription detail as current resource state rather than a result correlated to an acceptance receipt. Its first interface SHALL NOT include automatic activation/removal waiters or command-status polling. Examples SHALL separate command submission, inspection, and application-side event consumption.

#### Scenario: Existing active resource follows a new command
- **WHEN** a caller submits a command and then reads a previously active subscription
- **THEN** the SDK SHALL return the observed state without claiming that the new command succeeded

#### Scenario: Demo uses existing infrastructure
- **WHEN** an application-team example demonstrates subscribing, inspecting, publishing, and unsubscribing
- **THEN** it SHALL state the preconfigured domain, schema, binding, entitlement, and event-consumption prerequisites
- **AND** it SHALL NOT silently provision resources, acquire elevated roles, or label command acceptance as readiness
