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

## `materialize-subscription-env` (`follow-schema-promotion`, 2026-09-08; `mode: repin` added by `promote-schema-versions`, 2026-02-11)

- **Committed writes:** one `UpdateItem` per invocation—the per-env merge (`SET environments.#env = :entry, updatedAt = :now`) recording the materialization outcome (ACTIVE with `ruleArn`/`pinnedVersion`, AWAITING_PROMOTION, or FAILED with `errorDetail`).
- **Side effects:** before the commit, in order—`DescribeSchema` (read), entitlements query (read, skipped for disabled envs and self-subscriptions), `PutRule` upsert, `PutTargets` (consumer bus + platform DLQ), `PutMetricAlarm` (zero-events). No compensation between steps.
- **Rubric:** (1) yes—the merged entry carries everything a reader or retry needs, including the failure step; (2) inverted—the side effects *precede* the commit, so a crash between them leaves an orphaned rule/target/alarm with no record entry; tolerated because the rule name is deterministic (`{consumer}-{detailType}`) so unsubscribe deletes it without a record entry, `update_state` enable re-invokes materialization as the heal path, and an orphan rule with no matching schema versions is inert; (3) yes—every AWS call is an upsert and the merge replaces its own sub-map, so re-invocation converges (asserted by the repeated-invocation test); (4) yes—subscription activation is explicitly asynchronous; (5) n/a—no stream consumer; (6) yes—the invoking Step Functions execution (activation workflow map iteration or promotion extender) is the durable envelope; transient AWS errors are recorded as per-env FAILED rather than retried, because the heal path exists and retry semantics stay in the caller.
- **Decision:** ENVELOPE-DRIVEN.
- **Rationale:** the caller's execution context already provides durability; the handler's single commit records the outcome and is the read surface for the workflow's post-map aggregation and the UI's failure display.

`mode: repin` addendum: the re-pin path performs no schema lookup, entitlement check, target, or alarm write—one `DescribeRule` (to preserve the rule's current state) and one `PutRule` with the caller-supplied version list, then, only when `commitPin` is set, a `SET environments.#env.pinnedVersion` update that touches nothing else in the entry. A `PutRule` failure returns a per-entry FAILED outcome, and—again only when `commitPin` is set—also marks the entry `FAILED` with `errorDetail` on the record, so the failing subscriber is visible and re-enabling repairs it; during widen a failure leaves the old pattern delivering and the workflow retries instead. The narrow-phase record write is best effort: it must not mask the step outcome the workflow tallies. The invoking workflow (promotion or re-pin) is the durable envelope, exactly as for the create path; the REPIN item's generation condition (written by the caller, `schema-promotion-steps`) is the concurrency guard.

## `subscription-extender` (`follow-schema-promotion`, 2026-09-08; re-pin trigger added by `promote-schema-versions`, 2026-02-11)

- **Committed writes:** none—the record merge for each extended environment is committed by `materialize-subscription-env`, which the extender invokes per subscription.
- **Side effects:** per eligible stream, a synchronous `lambda:InvokeFunction` (the materialization command); on a SUCCEEDED outcome, `SubscriptionEnvironmentExtended` on the control-plane bus.
- **Rubric:** (1) n/a—the durable record of extension is the materialize function's per-env merge, not anything the extender writes; (3) yes—a replayed promotion event finds extended environments already materialized and skips them (no re-invoke, no duplicate event); (6) yes—the control-plane bus rule's DLQ is the durable envelope, and invocation-level failures (SDK error, function error) propagate so EventBridge retries the event.
- **Failure walk:** query/lookup/invoke transient failure → the event fails and retries; a repeatedly crashing invocation dead-letters for replay after repair (materialization is idempotent, so replay is safe). A materialization *step* failure is already recorded as per-env FAILED by the invoked function, so it does not fail the event—the `update_state` heal path owns it. A missing consumer domain record is logged and counted, not retried (data-integrity problem, not transient). A lost `SubscriptionEnvironmentExtended` emission is logged-only: the event is observational, and a replay would find the environment materialized and skip without re-emitting—accepted.
- **Re-pin addendum:** when the published environment is the authoring environment, the extender also starts the `repin-schema-env` workflow as `repin-{domain}-{eventName}-{env}-v{version}`. The execution name is the idempotency key (`ExecutionAlreadyExists` is success), the committed schema version it reacts to is durable, and the start runs after the extension loop so a start failure fails the event into the retry path with extension already converged. Non-authoring environments never start it—their pins advance through promotion.
- **Decision:** ENVELOPE-DRIVEN.
- **Rationale:** the handler performs no commit of its own; the bus rule plus the invoked function's idempotent merge provide durability, and the lifecycle event hangs on the SUCCEEDED outcome the function committed.
