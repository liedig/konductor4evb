# Domain-management write paths

[Documentation home](../../README.md)

Dated records from the [side-effects audit](README.md). These are rationale and failure analysis, not proof that the present code still has this shape.

## `domain-registrations/register.rs` — registration path

- **Committed writes:** `TransactWriteItems`—put `PENDING_APPROVAL` row conditioned `attribute_not_exists(domainName)` + domain-count counter increment below the cap; on name-taken-by-REJECTED, a second transaction overwrites the row without charging the counter.
- **Side effects:** `start_execution` (approval workflow, name `approval-{requestId}`), then `RegisterDomainSucceeded` on the control-plane bus. Denial paths (authorization, validation, duplicate, at-cap) emit `RegisterDomainFailed` with no committed write.
- **Rubric:** (1) yes—the registration item carries every field the workflow input and both events need (requestId, owners, description, costCenter, environments, createdAt); (2) mixed—the workflow start and `RegisterDomainSucceeded` hang on the `PENDING_APPROVAL` commit, but the four denial emissions never commit a row and stay on an explicit emission path by design; (3) yes—marker-claim on stream event id, per the entitlements consumer pattern; (4) yes—registration is already fully asynchronous (API→SQS, 202); (5) requires a new domains-table stream consumer with bisect/retry/DLQ/IteratorAge alarms—established pattern, and the stream consumer must filter the `META#domain-count` counter item; (6) partially—the inbound SQS command is a durable envelope and the hand-rolled requestId continuation covers every crash window *while redelivery lasts*, but a message that exhausts redrive leaves a committed `PENDING_APPROVAL` row with no workflow and no event, recoverable only by manual DLQ redrive.
- **Failure walk:** crash between commit and workflow start, or between start and event emission → SQS redrive replays with the same requestId, the name-taken transaction returns the existing row, requestId match classifies it as our own earlier delivery, and the remaining steps rerun (execution name idempotent, event re-emitted). Crash after a successful emission → duplicate `RegisterDomainSucceeded` (unmitigated today). Redrive exhaustion → stranded row (uncovered).
- **Decision:** MIGRATE.
- **Rationale:** the item satisfies the rubric fully, the path dual-writes across three systems with hand-rolled continuation to compensate, and a stream makes the workflow start and outcome event crash-independent by construction—including the DLQ-terminal case the current recovery cannot cover. The stream record is a change record; the consumer translates it into the existing `RegisterDomainSucceeded`/collision-`RegisterDomainFailed` business events, so event contracts are unchanged.
- **Follow-up:** `stream-driven-domain-registration` (introduces the domains-table stream + consumer; must filter the counter item; carries the collision-preflight path below).

## `domain-registrations/register.rs` — collision-preflight path

- **Committed writes:** same two-phase transaction writing a `REJECTED` row (with `justification`) and charging the counter.
- **Side effects:** `RegisterDomainFailed` carrying the collision justification after the commit; duplicate/limit races reclassify and emit their own failure reasons.
- **Rubric:** (1) yes—the rejected row persists the justification, so the failure event is fully derivable from the record; (2) yes—here the rejection IS the committed transition; (3)–(5) as above; (6) as above.
- **Failure walk:** commit → crash before emission → redrive reclassifies (requestId match) and re-emits; covered while redelivery lasts, same DLQ-terminal residue as the registration path.
- **Decision:** MIGRATE.
- **Rationale:** identical mechanics to the registration path; the rejected-row insert is exactly the "committed state transition a rejection can hang on" the rubric asks for.
- **Follow-up:** `stream-driven-domain-registration`.

## `domain-registrations/update.rs`

- **Committed writes:** conditional `UpdateItem` (`attribute_exists` + `status = ACTIVE`, `SET` metadata fields + fresh `updatedAt`).
- **Side effects:** `UpdateDomainSucceeded` after the commit; `UpdateDomainFailed` for denials/not-found/not-active (no committed write).
- **Rubric:** (1) mostly—the written item carries the new values and `updatedAt`; `changedFields` is derivable only by diffing stream images, so the consumer needs old+new images; (2) mixed—success event hangs on the commit, failure events do not and stay explicit; (3) yes; (4) yes—SQS-async; (5) same domains-table consumer as registration; (6) partially—the SQS envelope drives retry, but the conditional write succeeds on every retry, so each redelivery re-bumps `updatedAt` and re-emits.
- **Failure walk:** commit → crash before emission → retry re-updates (new `updatedAt`) and emits; duplicate `UpdateDomainSucceeded` with differing `updatedAt`—uncovered today, and a naive stream port would reproduce it (each retry's `updatedAt` bump is itself a MODIFY record). The migration must make the write value-idempotent (skip the write when nothing changed) or the consumer content-aware.
- **Decision:** MIGRATE.
- **Rationale:** the duplicate-emission window is real and unmitigated; the stream gives one record per actual change, which is the correct event granularity. Failure events remain handler-emitted (no committed transition).
- **Follow-up:** `stream-domain-lifecycle-events` (shares the domains-table stream with `stream-driven-domain-registration`; whichever lands first builds the consumer infrastructure, the other adds translations).

## `update.rs` costCenter → retag workflow (`editable-cost-center`, 2026-09-01)

- **Committed writes:** none new—the same conditional `UpdateItem` audited above, now also setting `costCenter` and recomputing the stored `tags` map atomically when `costCenter` changes. The retag workflow performs no DynamoDB writes.
- **Side effects:** `UpdateDomainSucceeded` (now carrying `requestId` and, when changed, `costCenter`) is matched by an EventBridge rule that starts the `konductor-retag-domain` STANDARD workflow; the workflow issues tag upserts on every recorded provisioned resource (platform account directly, owner account via the trust role) and emits `RetagDomainFailed` on unrecoverable failure.
- **Rubric:** (1) yes—the committed record carries everything the workflow needs (`costCenter`, `environments`, `resourceArns`, `ownerAccountId`); the workflow reads the record rather than trusting the event beyond `domainName`/`requestId`; (2) yes—the rule fires only on `UpdateDomainSucceeded`, which hangs on the committed transition; rejections never commit and never reach the rule; (3) partially—EventBridge rule targets cannot set Step Functions execution names (StartExecution takes `name` only as a top-level API parameter; aws/aws-cdk#34731 tracks the gap), so executions are auto-named and a redelivered event can start a duplicate; tolerated because every tag call is an idempotent upsert against the record's value, so duplicates converge to identical state; (4) yes—retagging is explicitly eventually consistent; (5) n/a—no stream consumer involved; (6) yes—the STANDARD execution context is the durable envelope for the tag calls, and the rule's DLQ covers target-invocation failure.
- **Failure walk:** event redelivery → duplicate execution re-applies the same tags (harmless); tag call exhausts retries → iteration error recorded, remaining environments still converge, `RetagDomainFailed` identifies the first failure; rule target invocation fails outright → rule DLQ, redrivable.
- **Decision:** STAY—no new synchronous side effect in the handler (single commit, one event); convergence is workflow-envelope-driven. When `stream-domain-lifecycle-events` lands, the rule can be re-pointed at stream-derived events without changing the workflow contract.
- **Follow-up:** —


## `update.rs` ownerEmail → topic subscription reconciliation (`domain-system-events`, 2026-10-21)

- **Committed writes:** none new—the same conditional `UpdateItem` audited above; a `GetItem` before the update reads the replaced `ownerEmail` so the event can name it.
- **Side effects:** `UpdateDomainSucceeded` (carrying `ownerEmail` and `previousOwnerEmail` when the owner changed) is matched by an EventBridge rule that invokes `system-topic-manager` (`reconcileOwnerEmail`); the function subscribes the new owner address to the domain's system-events topic and unsubscribes the previous one. SNS effects only; no DynamoDB write.
- **Rubric:** (1) yes—the event carries both addresses and the topic ARN is deterministic from organization and domain name; (2) yes—the rule fires only on the post-commit success event; (3) yes—reconciliation is idempotent (an existing subscription is reused, an absent previous address is a no-op), so redelivery converges; (4) yes—subscription setup tolerates delay and the owner confirms asynchronously; (5) n/a—no stream consumer involved; (6) yes—the rule's retry policy and DLQ cover target-invocation failure.
- **Failure walk:** target invocation fails → rule DLQ, redrivable; SNS call exhausts → function error → DLQ; a still-pending old subscription cannot be unsubscribed (SNS reports `PendingConfirmation` instead of an ARN) and is left to expire; two rapid owner changes can strand an intermediate address until it expires or an operator removes it—accepted and documented in the change design.
- **Decision:** STAY.
- **Follow-up:** —

## `domain-state` — retry-provisioning

- **Committed writes:** conditional `UpdateItem` (`PROVISIONING_FAILED` → `APPROVED`, removes `errorDetail`).
- **Side effects:** `DomainProvisioningRetryRequested` (carries item fields + `retriedBy`, `retriedAt`), which triggers the provisioning saga. Enable/disable transitions emit nothing.
- **Rubric:** (1) no—`retriedBy` is handler-local and never persisted; a stream consumer loses actor attribution (migration must add it to the item); (2) yes—the event hangs on a committed transition; (3) yes; (4) yes—provisioning takes minutes; (5) as above; (6) **no**—this is a synchronous API path with no inbound envelope at all.
- **Failure walk:** commit → emission fails → the caller's natural retry 409 s (status no longer `PROVISIONING_FAILED`) and the event never goes out: the domain sits in `APPROVED` with no provisioning execution, permanently wedged without manual intervention. **Uncovered today—the worst gap in the audit.**
- **Decision:** MIGRATE.
- **Rationale:** the only handler with an uncovered partial state *and* no durable envelope; the rubric's question 6 fails outright. Migration closes the wedge by construction.
- **Follow-up:** `stream-domain-lifecycle-events`.

## `domain-state` — enable/disable

- **Committed writes:** single conditional `UpdateItem` per transition.
- **Side effects:** none (deliberately shallow).
- **Rubric:** trivially satisfied—one commit, zero side effects; nothing to migrate.
- **Decision:** STAY. **Rationale:** already the rubric's shape.

## `finalize-approval`

- **Committed writes:** one `TransactWriteItems`: domain `PENDING_APPROVAL` → `APPROVED` (with `decidedBy`, `justification`, `approvedAt`) + up to 3 conditional binding puts + 3 audit-record puts.
- **Side effects:** none—outcome events (`DomainRegistrationApproved` and so on) are emitted by the enclosing approval state machine, which is also what starts provisioning.
- **Rubric:** (1)–(5) moot—nothing to move; (6) the Step Functions execution context is the durable envelope.
- **Failure walk:** crash before commit → SFN task retry; crash after commit → retry cancels on the `PENDING_APPROVAL` condition and maps to already-finalized success. Atomic, covered.
- **Decision:** STAY. **Rationale:** one atomic commit, no handler-emitted side effects; the workflow owns downstream effects.

## `decide-registration`

- **Committed writes:** none—no DynamoDB client in the handler.
- **Side effects:** exactly one `send_task_success`/`send_task_failure` per request.
- **Failure walk:** crash after the callback → caller retry gets `TaskDoesNotExist` → 409 AlreadyProcessed. Covered.
- **Decision:** STAY. **Rationale:** not a write path; the single-use task token is the idempotency mechanism.

## `send-approval-request-notification`

- **Committed writes:** conditional `UpdateItem` persisting the Step Functions `taskToken` onto the domain record.
- **Side effects:** SES email to the platform operator, sent after the write; body derives from the workflow payload plus an `is_new_account` GSI query.
- **Rubric:** (1) no—the email content comes from the workflow payload, not the item, and streaming the bearer task token to a consumer would widen its exposure for no gain; (2) no—the token write is plumbing, not a business transition; (4) yes; (6) the invoking state machine is the durable envelope (task retry + Catch routing).
- **Failure walk:** crash between write and send → SFN retry re-writes the same token (idempotent) and re-sends; crash after send → duplicate operator email (tolerated). **Noted risk, orthogonal to streams:** on a *permanent* SES failure the handler deliberately returns success, so the workflow waits the full 7-day timeout for a decision that can never arrive, and only then routes to `APPROVAL_NOTIFICATION_FAILED`. Consider a shorter "token persisted but no decision" alarm.
- **Decision:** STAY. **Rationale:** the state machine orchestrates this step and owns its recovery; the write exists only to serve the workflow's callback contract.

## Outcome/notification email handlers (send-approval-outcome, send-rejection-outcome, send-registration-failed, send-provisioning-failed, send-provisioning-succeeded, send-registration-delayed)

- **Committed writes:** none (GetItem reads at most, for recipient resolution).
- **Side effects:** one SES email per invocation; permanent SES failures are deliberately acknowledged (log-only) rather than retried.
- **Rubric:** (6) yes—the triggering EventBridge business event is the durable envelope, with the rule target's DLQ as the terminal backstop.
- **Failure walk:** the only window is send-then-crash → duplicate email on redelivery; tolerated (no sent-state tracking anywhere). Swallowed permanent failures never reach the DLQ—observable in logs only; a standing trade-off, not a stream concern.
- **Decision:** ENVELOPE-DRIVEN. **Rationale:** these are already the "consumer reacts to a business event" shape the rubric wants; nothing commits locally.
