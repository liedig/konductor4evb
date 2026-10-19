# Spec Delta

## ADDED Requirements

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

## MODIFIED Requirements

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
