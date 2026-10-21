# Subscription write paths

[Documentation home](../../README.md)

Dated records from the [side-effects audit](README.md). These are rationale and failure analysis, not proof that the present code still has this shape.

## `subscriptions/subscribe.rs`

- **Committed writes:** unconditional `PutItem` of the stream record (`PENDING`, per-env entries, `requestId = SQS messageId`), guarded only by a non-atomic read-before-write idempotency check.
- **Side effects:** `start_execution` of the activation workflow (name `activate-{messageId}`, idempotent per message), then `SubscribeSucceeded` (`idempotent: true` variant on replay). Denials emit `SubscribeFailed` with no committed write.
- **Rubric:** (1) yes—the record carries every field of the workflow input except `consumerAccountId`, which a consumer may read from the durable domain record; (2) mixed, as with registration—success hangs on the `PENDING` commit, failures stay explicit; (3) yes; (4) yes—SQS-async; (5) needs an event-streams-table stream consumer (the table has TTL'd rows; the stream sees them, which is fine—inserts/MODIFYs are what matter); (6) partially—the envelope exists and execution names dedupe same-message redeliveries, but see the failure walk.
- **Failure walk:** commit → crash before `start_execution` → redelivery hits the idempotency check, sees `PENDING`, emits an idempotent success and acks **without ever starting the workflow**—the subscription is stranded with no rule created, uncovered. Two concurrent subscribes can both pass the check-then-put (last-writer-wins on the item, two executions against one record). Post-start crash → covered (replay emits idempotent success).
- **Decision:** MIGRATE.
- **Rationale:** the stranded-PENDING window is an uncovered partial state produced by conflating "record exists" with "workflow started"; a stream consumer starting the workflow from the committed record removes the ordering dependency entirely, and the migration naturally replaces the non-atomic check-then-put with a conditional put. `SubscribeSucceeded` remains a business event translated from the committed change record; `SubscribeFailed` stays handler-emitted.
- **Follow-up:** `stream-subscription-outcomes`.

## `subscriptions/update_state.rs`

- **Committed writes:** per requested environment: EventBridge `EnableRule`/`DisableRule` followed by unconditional `UpdateItem` (`environments.{env}.enabled`), looped sequentially.
- **Side effects:** one aggregate `UpdateSubscriptionSucceeded` after the loop; `UpdateSubscriptionFailed` for denials.
- **Rubric:** (1) no—the success payload is the *requested* enabled map (handler-local), and the loop commits N independent per-env transitions, so no single stream record corresponds to the aggregate outcome; (2) no—the aggregate event hangs on no single committed transition; (3)–(4) fine; (6) yes—SQS envelope, and every step is idempotent-by-value.
- **Failure walk:** die anywhere mid-loop → redelivery replays the whole loop; rule flips and updates converge by value. All windows covered.
- **Decision:** STAY. **Rationale:** recovery already converges under the existing envelope, and the rubric's first two questions fail—a stream would emit N MODIFY records with no way to reconstruct the one business event.

## `subscriptions/unsubscribe.rs`

- **Committed writes:** none in the handler (the removal workflow marks the record `REMOVED` with a 90-day TTL).
- **Side effects:** `start_execution` of the removal workflow, then `UnsubscribeSucceeded`; denials emit `UnsubscribeFailed`.
- **Failure walk:** crash between start and event → redelivery re-reads the record (not yet `REMOVED`—the workflow just started) and starts a **second** execution: the execution name embeds a unix timestamp (`remove-{id}-{ts}`), defeating Step Functions name idempotency. Duplicate removal executions are possible; whether the workflow tolerates them is unverified.
- **Decision:** STAY.
- **Rationale:** there is no committed write in this handler for a stream to carry—the fix is not a migration but restoring the established execution-name idempotency convention.
- **Follow-up:** `fix-unsubscribe-execution-idempotency` (deterministic name, for example, `remove-{subscriptionId}` or the SQS messageId; verify the removal workflow's reentrancy).

## `materialize-subscription-env` (`follow-schema-promotion`, 2026-09-08; `mode: repin` added by `promote-schema-versions`, 2026-02-11; upgrade phase commits added by `domain-system-events`, 2026-10-20)

- **Committed writes:** one `UpdateItem` per invocation—the per-env merge (`SET environments.#env = :entry, updatedAt = :now`) recording the materialization outcome (ACTIVE with `ruleArn`/`pinnedVersion`, AWAITING_PROMOTION, or FAILED with `errorDetail`).
- **Side effects:** before the commit, in order—`DescribeSchema` (read), entitlements query (read, skipped for disabled envs and self-subscriptions), `PutRule` upsert, `PutTargets` (consumer bus + platform DLQ), `PutMetricAlarm` (zero-events). No compensation between steps.
- **Rubric:** (1) yes—the merged entry carries everything a reader or retry needs, including the failure step; (2) inverted—the side effects *precede* the commit, so a crash between them leaves an orphaned rule/target/alarm with no record entry; tolerated because the rule name is deterministic (`{consumer}-{detailType}`) so unsubscribe deletes it without a record entry, `update_state` enable re-invokes materialization as the heal path, and an orphan rule with no matching schema versions is inert; (3) yes—every AWS call is an upsert and the merge replaces its own sub-map, so re-invocation converges (asserted by the repeated-invocation test); (4) yes—subscription activation is explicitly asynchronous; (5) n/a—no stream consumer; (6) yes—the invoking Step Functions execution (activation workflow map iteration or promotion extender) is the durable envelope; transient AWS errors are recorded as per-env FAILED rather than retried, because the heal path exists and retry semantics stay in the caller.
- **Decision:** ENVELOPE-DRIVEN.
- **Rationale:** the caller's execution context already provides durability; the handler's single commit records the outcome and is the read surface for the workflow's post-map aggregation and the UI's failure display.

`mode: repin` addendum: the re-pin path performs no schema lookup, entitlement check, target, or alarm write—one `DescribeRule` (to preserve the rule's current state) and one `PutRule` with the caller-supplied version list, then, only when `commitPin` is set, the narrow-phase record commit. A `PutRule` failure returns a per-entry FAILED outcome, and—again only when `commitPin` is set—also marks the entry `FAILED` with `errorDetail` on the record, so the failing subscriber is visible and re-enabling repairs it; during widen a failure leaves the old pattern delivering and the workflow retries instead. The invoking workflow (promotion or re-pin) is the durable envelope, exactly as for the create path; the REPIN item's generation condition (written by the caller, `schema-promotion-steps`) is the concurrency guard.

`mode: repin` upgrade-phase commits (2026-10-20): when the workflow enumerates entries with upgrade facts (system-event emission enabled), the record writes change shape:

- **Committed writes:** widen: a conditional `UpdateItem` committing the `STARTED` snapshot to `upgradeEvents.{env}` *before* any rule change, guarded by the generation-derived resource revision plus an eligibility re-check (entry exists and `ACTIVE`, stream not `REMOVED`). Narrow: one conditional `UpdateItem` landing the pin (`environments.{env}.pinnedVersion`) and the `TERMINAL` snapshot together; on rule failure, one conditional `UpdateItem` landing the `FAILED` marker and the terminal failure snapshot together. Without upgrade facts the writes stay exactly as before (`SET ... pinnedVersion` / `set_env_failed`).
- **Rubric:** (1) yes—the snapshot is self-contained: operation id, trigger, generation, revision, idempotency key, version facts, contract selectors, and enablement; the stream projector needs no other read; (2) yes—both phase commits hang on committed conditional transitions, and the terminal snapshot is atomic with the business state it describes; (3) yes—the idempotency key is operation- and phase-unique (`system-event/v1/` over org, subscription id, environment, operation id, phase), and a conditional-check failure rereads the record to classify `AlreadyCommitted`/`Superseded`/retry; (4) yes—seconds of stream latency are the design (consumers poll or subscribe to topics, never synchronously); (5) yes—the projector (group 3) is a standard stream consumer with per-record events well under shard limits; (6) yes—the invoking Step Functions execution remains the durable envelope.
- **Failure walk:** widen: snapshot commit rejected because a newer revision exists → step returns SUPERSEDED and the rule is untouched (the newer widen owns the entry); ambiguous conditional failure → reread: own key present → AlreadyCommitted → proceed (the retry-after-commit case loses no event); entry gone/removed → SUPERSEDED, no fabricated terminal. Narrow: pin+snapshot commit fails conditionally → same reread classification; duplicate commit after ambiguous failure → AlreadyCommitted → REPINNED. Rule failure → failure marker + terminal snapshot commit is *required and retriable* (it replaced the previous best-effort write): a write error now fails the step so the workflow retries instead of losing the failure record; a superseded record skips the marker without masking the FAILED outcome. A pin below the requested version commits `PARTIAL`/`REPLAY_PARTIAL` truthfully rather than claiming completion.
- **Decision:** ENVELOPE-DRIVEN (unchanged).

## `subscription-extender` (`follow-schema-promotion`, 2026-09-08; re-pin trigger added by `promote-schema-versions`, 2026-02-11)

- **Committed writes:** none—the record merge for each extended environment is committed by `materialize-subscription-env`, which the extender invokes per subscription.
- **Side effects:** per eligible stream, a synchronous `lambda:InvokeFunction` (the materialization command); on a SUCCEEDED outcome, `SubscriptionEnvironmentExtended` on the control-plane bus.
- **Rubric:** (1) n/a—the durable record of extension is the materialize function's per-env merge, not anything the extender writes; (3) yes—a replayed promotion event finds extended environments already materialized and skips them (no re-invoke, no duplicate event); (6) yes—the control-plane bus rule's DLQ is the durable envelope, and invocation-level failures (SDK error, function error) propagate so EventBridge retries the event.
- **Failure walk:** query/lookup/invoke transient failure → the event fails and retries; a repeatedly crashing invocation dead-letters for replay after repair (materialization is idempotent, so replay is safe). A materialization *step* failure is already recorded as per-env FAILED by the invoked function, so it does not fail the event—the `update_state` heal path owns it. A missing consumer domain record is logged and counted, not retried (data-integrity problem, not transient). A lost `SubscriptionEnvironmentExtended` emission is logged-only: the event is observational, and a replay would find the environment materialized and skip without re-emitting—accepted.
- **Re-pin addendum:** when the published environment is the authoring environment, the extender also starts the `repin-schema-env` workflow as `repin-{domain}-{eventName}-{env}-v{version}`. The execution name is the idempotency key (`ExecutionAlreadyExists` is success), the committed schema version it reacts to is durable, and the start runs after the extension loop so a start failure fails the event into the retry path with extension already converged. Non-authoring environments never start it—their pins advance through promotion.
- **Decision:** ENVELOPE-DRIVEN.
- **Rationale:** the handler performs no commit of its own; the bus rule plus the invoked function's idempotent merge provide durability, and the lifecycle event hangs on the SUCCEEDED outcome the function committed.

## `system-event-router` (updated by `domain-system-events`, 2026-10-21)

- **Committed writes:** one conditional `UpdateItem` on the domains table per
  routed event, setting `systemEventsLastEventAt`/`systemEventsLastEventType`
  (the console's unread-indicator marker). The write runs after the SNS
  publish it records and hangs on the same `konductor.system` bus event that
  drove the publish; the router performs no other writes.
- **Rubric:** (1) n/a — the marker is console-facing freshness metadata, not
  state a stream consumer needs (the domains table has no stream, so no
  consumer can react to it); (2) yes — the stamp hangs on the delivered
  system event, which the projector emitted from a committed snapshot
  transition; (3) yes — a retried delivery re-publishes the byte-identical
  event and re-stamps the same timestamp; the condition
  (`attribute_not_exists(...) OR ... <= :at`) makes superseded stamps a
  no-op; (4) yes — the marker may lag seconds without violating anything,
  and a lost stamp only means a stale unread dot; (5) n/a — no stream
  consumption; (6) yes — the bus rule's retry/DLQ envelope is the durable
  source, and the stamp is a pure function of that envelope's payload.
- **Failure walk:** conditional-check failure (an equal or newer marker
  already recorded) → silent no-op. Any other `UpdateItem` error (throttle,
  dispatch, timeout) → warn-only; the handler never fails or republishes an
  event over a marker, and the next event self-heals the timestamp.
- **Decision:** STAY. **Rationale:** the marker must land on the domain
  record the console already reads, synchronously with delivery; no stream
  exists on the domains table to drive it from, and adding one to carry
  cosmetic freshness would be complexity without a consumer. The write is
  idempotent, newer-wins, and strictly best-effort.
## `system-event-projector` (added by `domain-system-events`, 2026-07-30)

- **Committed writes:** none of its own — it consumes the event-streams table's
  stream (`NEW_AND_OLD_IMAGES`) and diffs old/new `upgradeEvents` snapshots.
  Its only effect is `PutEvents` of the public `konductor.system` envelope onto
  the system bus for snapshots whose idempotency key or resource revision
  advanced.
- **Rubric:** (1) yes — the stream image carries the complete snapshot the
  public event is built from; (2) yes — every event hangs on the committed
  conditional write in `materialize-subscription-env`, never on a workflow
  response; (3) yes — the event's deterministic `idempotencyKey` and
  `resourceRevision` are stable across retries, redrives, and replays, so a
  re-publication is byte-identical and consumers deduplicate on it; (4) yes —
  notification latency is seconds-tolerant by contract; (5) yes — the event
  source mapping bisects, retries boundedly (3 attempts, one-hour record age),
  reports partial batch failures, and delivers the full failed payload to an
  S3 bucket with 90-day retention, so a poison record cannot block the shard
  past the 24-hour stream window; (6) yes — the stream record itself is the
  durable envelope.
- **Failure walk:** bus throttled or an entry rejected → the record fails and
  retries, then bisects, then lands in the failure bucket with the original
  payload; the [recovery runbook](../../operations/system-events.md) re-invokes
  the projector with the extracted records and identity is preserved. A
  malformed snapshot entry fails the record loudly (never a silent skip). An
  out-of-order delivery whose old image already holds a newer resource
  revision emits nothing. REMOVE records and ordinary mutations without
  snapshot changes emit nothing.
- **Decision:** Reference (exempt). **Rationale:** a pure stream consumer in
  the pattern the rubric exists to promote; no synchronous fan-out on any
  write path.

## `system-event-notifier` (added by `domain-system-events`, 2026-07-30)

- **Committed writes:** none of its own — it is an EventBridge rule target on
  the system bus (consumer-facing events) and the control-plane bus
  (`SchemaPromotionFailed`). Its only effects are a `GetItem` for recipient
  lookup and `ses:SendEmail`.
- **Rubric:** (1) n/a — it writes nothing; (2) yes — both triggering events
  hang on committed state transitions (the conditional upgrade-phase write
  via the projector; the committed failure record for promotion); (3) yes —
  the event's deterministic `idempotencyKey` makes a redelivered email
  byte-identical, and EventBridge target retries plus the rule DLQ bound
  duplicates to transient windows; (4) yes — email is advisory, seconds of
  delay are within contract; (5) n/a — no stream consumption; (6) yes — the
  rule's DLQ (`SystemEventTargetDlq`) plus two target retries is the durable
  envelope.
- **Failure walk:** transient SES failure (throttling) → returned to
  EventBridge, retried, then dead-lettered for targeted redrive per the
  [operations guide](../../operations/system-events.md). Permanent SES
  failure (`MessageRejected`, `BadRequestException` — e.g., an unverified
  sender) is acknowledged without retry: retrying cannot help, and machine
  consumers are unaffected. A missing domain record skips the email with a
  log line. Emails are never sent for authoring-triggered upgrades.
- **Decision:** ENVELOPE-DRIVEN. **Rationale:** a read-plus-send consumer
  whose durability comes from the bus rule's retry/DLQ envelope; identical
  event payloads make duplicate sends harmless to dedupe.
