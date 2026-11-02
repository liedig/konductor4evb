# Design

## Context

See proposal.md (Why) for the benchmark and the bottlenecks. The current shape that constrains the approach:

- **Admission** (`publish-events-admission`) is an `aws_proxy` Lambda on both API surfaces. It authorizes through the grant engine, reads the domain with an uncached `GetItem`, resolves the latest schema through `SchemaCache` (TTL ≤ 60 s, refresh before reject), and makes one `SendMessage`. It already logs `AdmissionAccepted` with `latency_ms`. It runs under `ReservedConcurrentExecutions: 100`.
- **Delivery** (`publish-events`) is an SQS consumer with `BatchSize: 10` and `ReportBatchItemFailures`, and no concurrency bound. It handles records in a serial loop: parse, uncached domain `GetItem`, build the envelope, validate composed metadata, then one single-entry `PutEvents`. Every failure is reported for retry.
- **Queue:** `PublishQueue` has `VisibilityTimeout: 30` and `maxReceiveCount: 3`, with `PublishDlq` as its dead-letter queue (no alarm). `PublishQueueAdmissionOnlyPolicy` denies `sqs:SendMessage` to everyone except admission. It does not affect `ChangeMessageVisibility`.
- **Stage throttles:** one `MethodSettings` entry (`/*`, `*`, 50 rps, burst 100) on each authoring stage, and on every rendered stage. `render_stages_block` in `scripts/validate-config.py` emits the rendered stages from the environment list alone.
- **Settings:** `SETTINGS` in `validate-config.py` maps each snake_case setting to a template parameter and a lifecycle class. `--render` prints the `sam deploy --parameter-overrides` value. The AWS checks (SES status, drift guard) call the `aws` CLI through `subprocess`, take fixture flags in tests, and are skipped with `--skip-aws-checks`. The script has `err()` and `note()` helpers but no warning level.
- **Global Lambda defaults:** a 30 s timeout and 256 MB of memory. The workspace already depends on `tokio` (full) and `uuid` (v4), and has no `futures` or `rand` crate. `envelope.rs` exposes `PUT_EVENTS_ENTRY_MAX_BYTES`.

## Goals / Non-Goals

**Goals:**

- Replace the fixed limits Konductor sets on itself with operator settings. The defaults keep publish traffic from using up the account's shared API Gateway throttle, and otherwise only operator-raisable AWS quotas bound the publish path.
- Cut per-event delivery cost and latency by batching, and cut admission latency by caching domain liveness and pre-warming connections.
- Under sustained EventBridge throttling, build a delayed backlog and never dead-letter valid events.
- Change no wire contract: the request and response bodies, the queue message format (legacy and format 2), and the delivered envelope all stay byte-for-byte the same.

**Non-Goals:**

- Admission-side changes beyond the domain liveness cache and pre-warm. The authorization path, the schema cache, and the metadata pointer read stay as they are.
- A distributed or shared cache (ElastiCache, DAX), or invalidating the cache when a domain is disabled.
- FIFO ordering or exactly-once delivery. The path stays at-least-once, and consumers dedupe on `idempotencyKey`.
- Re-tuning Lambda memory, delivery `BatchSize`, or the batching window.

## Decisions

### 1. Publish throttle as a method-level setting that references parameters

Each authoring stage and each rendered stage gets a second `MethodSettings` entry: `ResourcePath: "/~1publish"`, `HttpMethod: POST`, with `ThrottlingRateLimit` and `ThrottlingBurstLimit` set by `Ref` to the new `PublishThrottleRateLimit` and `PublishThrottleBurstLimit` parameters. A method-level setting overrides the stage-wide `/*` entry for that method only, so every other route keeps 50 rps and a burst of 100. `render_stages_block` emits `{ Ref: PublishThrottleRateLimit }` rather than literal numbers. The rendered block therefore still depends only on the environment list, and changing a throttle value is a parameter update with no re-render.

- *Alternative: render literal values from the settings.* Rejected. A setting change would then also need a re-render, which ties the rendered block to the settings.
- *Alternative: usage plans.* Rejected. Usage plans need API keys and do not fit the OAuth and IAM callers.
- **Why 2,000 and 1,000:** stage and method throttles are ceilings, not reservations. Every REST, HTTP, and WebSocket API in the account shares one account throttle per Region. By default that is 10,000 rps with a burst of 5,000, and in some newer Regions it is 2,500 with a burst of 1,250. AWS sets the burst from the rate, and operators cannot request a different burst. With the defaults and three environments, one stage at its full publish rate plus every stage's control-plane limit comes to 2,300 rps with a burst of 1,600. That leaves more than 7,000 rps of the standard quota for the other stages, the console, the domain-state endpoint an operator needs to stop a runaway publisher, and other APIs in the account. 2,000 rps is twice the benchmark's 1,000 messages per second. Operators can raise it, and decision 9 warns at deploy time when a configured value no longer fits.
- *Alternative: the account defaults (10,000 rps, burst 5,000) as the per-stage default.* Rejected. A single stage could take the whole account bucket and starve every other route.

### 2. Admission concurrency cap is conditional, with 0 meaning uncapped

The new `PublishAdmissionConcurrencyCap` parameter (Number, default 0) drives a `HasPublishAdmissionConcurrencyCap` condition. `ReservedConcurrentExecutions` becomes `!If [HasPublishAdmissionConcurrencyCap, !Ref PublishAdmissionConcurrencyCap, !Ref AWS::NoValue]`. Removing the property makes CloudFormation delete the function's concurrency setting, so admission draws from the account's unreserved pool.

- *Alternative: raise the fixed cap, for example to 1,000.* Rejected. Reserved concurrency is subtracted from the shared pool even when idle, and any fixed number is a self-imposed limit.
- *Alternative: provisioned concurrency.* Rejected here. It adds cost and is out of scope.
- **Why 0:** Lambda reserved concurrency of 0 means "always throttle", so 0 can never be a useful cap. The validator rejects negative values. CloudFormation rejects caps above what the account can reserve.

### 3. Delivery concurrency uses the event source mapping's `MaximumConcurrency`, not reserved concurrency

`SqsTrigger` gets `ScalingConfig: { MaximumConcurrency: !Ref PublishDeliveryMaxConcurrency }`. The parameter defaults to 50, and the validator and the template both bound it to 2-1000, the AWS range. With `MaximumConcurrency`, the poller stops fetching when the limit is reached, so waiting messages are not received and their receive counts do not grow.

- *Alternative: reserved concurrency on delivery.* Rejected. The poller keeps receiving messages, Lambda throttles the invocations, and the messages go back to the queue with their receive count raised. That is the path into the DLQ that this change exists to close.
- **Why 50:** with 10 entries per `PutEvents` and invocations of tens of milliseconds, 50 concurrent invocations deliver far more than the default `PutEvents` quota in most Regions. EventBridge throttling therefore remains the binding limit, and the backoff below absorbs it without an invocation storm. Operators tune the value against their Region's quota.

`BatchSize` stays at 10, the `PutEvents` per-request entry limit, so a single-bus batch maps to one request. No batching window is added because it would add latency.

### 4. Batched delivery: prepare per record, send per bus, map failures by index

`handle` becomes three phases:

1. **Prepare.** For each record, run the existing parse, liveness check, envelope build, and composed-metadata validation, and produce either a ready entry (message ID, receipt handle, receive count, sent timestamp, bus name, `PutEventsRequestEntry`) or a per-message failure. Liveness checks are deduplicated per domain within the batch, through the cache in decision 6. The legacy and format-2 paths stay as they are and differ only in how they build the envelope.
2. **Group.** Group ready entries by bus name, then chunk each group so that every request has at most 10 entries and at most `PUT_EVENTS_ENTRY_MAX_BYTES` in total. Entry size uses EventBridge's documented accounting (time, source, detail-type, detail, and resources), which a new shared helper in `envelope.rs` provides next to the existing per-entry check.
3. **Send.** Send the chunks concurrently through a `tokio::task::JoinSet` (the SDK clients are cheap `Arc` clones). Map each response entry back to its message by index. A whole-request SDK error marks every entry of that chunk as failed with the error's code.

Each failure is then one of three outcomes:

- **Accepted:** the message is acknowledged (left out of `batchItemFailures`).
- **Throttled:** a request-level `ThrottlingException`, or an entry whose `ErrorCode` is `ThrottlingException`. These go to decision 5.
- **Other:** reported for retry with the standard timing.

- *Alternative: keep one request per record but send records concurrently.* Rejected. It keeps 10× the `PutEvents` calls against the quota, and the per-call overhead remains.
- *Alternative: add the `futures` crate for `join_all`.* Not needed, because `JoinSet` comes with the existing `tokio` dependency.

### 5. Throttle recovery: bounded in-invocation retry, then a visibility backoff, then a larger receive budget

- **In-invocation retry.** Only throttled entries are retried, grouped again by bus, up to 3 more attempts with full-jitter backoff (base 100 ms, cap 1 s). The retry stops once the invocation's remaining time drops below a safety margin (5 s). The SDK's standard retry still handles request-level throttling inside each attempt.
- **Visibility backoff.** For messages still throttled, delivery calls `ChangeMessageVisibilityBatch` on `PUBLISH_QUEUE_URL`, in groups of up to 10. The new timeout is `d/2 + jitter(0, d/2)` seconds with `d = min(60, 2^(n-1))`, where `n` is the record's `ApproximateReceiveCount`. The messages are then reported in `batchItemFailures`. The event source mapping leaves the visibility of failed messages alone, so the shorter timeout holds. If the visibility call fails, delivery logs it and falls back to the standard 30 s, which is still correct, just slower.
- **Receive budget.** `maxReceiveCount` rises from 3 to 25. Using the minimum backoff (half of `d`), receives 1-6 wait at least 31.5 s in total, and each later receive waits at least 30 s. 25 receives therefore cover at least 31.5 + 19 × 30 = 601.5 s of sustained throttling, plus the time spent retrying inside invocations. This meets the 10-minute bar in the spec.
- **Jitter source.** A small `JitterSource` trait in `konductor-common`, backed by uuid-v4 random bytes in production and by a deterministic value in tests. This avoids adding a `rand` dependency.
- *Alternative: rely on the visibility timeout alone, with a larger `maxReceiveCount`.* Rejected. A 30 s fixed retry gives a slow recovery and a synchronized retry herd.
- *Alternative: a separate retry queue with delay seconds.* Rejected. It is another queue to secure (the admission-only send policy), and the message would have to be re-sent, which breaks the queue-accepted `SentTimestamp` that sets `publishedAt`.

### 6. Shared, ACTIVE-only domain liveness cache

The new `konductor_common::domain_liveness` module provides `DomainLivenessCache`:

- **Storage:** a process-local `Mutex<HashMap<String, Instant>>` that records when each domain was last confirmed `ACTIVE`, with the window as a `Duration`.
- **API:** `confirm_active(domain, fetch)` returns the cached confirmation when it is younger than the window. Otherwise it calls `fetch`, which reads the domain and reports whether it is `ACTIVE`. It stores only positive results and passes through the caller's error type, so admission keeps `ControlPlaneError` (404 or 503) and delivery keeps `anyhow`.
- **Window 0:** a window of 0 never stores anything, so every check reads the domain.
- **Testing:** an injectable time source makes window expiry testable without sleeping.

Both functions read `DOMAIN_LIVENESS_CACHE_SECONDS`, set from the `DomainLivenessCacheSeconds` parameter (default 300, range 0-900). The cache sits in front of the existing `require_active_domain` reads in both handlers, and the authorization order and the response codes do not change.

- **Disable bound:** an instance can rely on a confirmation from before t0 only until that confirmation is one window old, so it stops admitting or delivering at t0 plus the window at the latest. This holds for admission and delivery separately, and that is the guarantee in the spec.
- *Alternative: invalidate on the domain-state change through the stream.* Rejected. Lambda execution environments cannot be addressed, so it would need polling or a shared cache.
- *Alternative: cache negative results too.* Rejected. A re-enabled or newly provisioned domain must work on the next publish.
- *Alternative: separate windows for admission and delivery.* Rejected. One setting is simpler and gives the same bound.

### 7. Best-effort init pre-warm in admission only

After building the clients, `main.rs` runs a `GetItem` on a reserved key that can never be a valid domain name, for example `#warmup`, on the domains table (the function already has `dynamodb:GetItem`). Concurrently, it runs `GetQueueAttributes(QueueArn)` on the publish queue, for which the role gains `sqs:GetQueueAttributes`. Both run under a 2 s `tokio::time::timeout`, and failures are logged and ignored, so init never fails or hangs on them. The pre-warm makes no Schemas API calls, to spare that quota.

- *Alternative: pre-warm delivery as well.* Not done. Delivery runs warm under a steady backlog, and its first-call cost is hidden by the time messages already spend in the queue.

### 8. Latency metrics from structured logs and metric filters

This follows the existing `metric = ...` and `AWS::Logs::MetricFilter` pattern, so no new SDK or IAM is needed. The metrics have no dimensions, so cardinality stays flat across up to 1,000 domains. Per-domain breakdowns come from Logs Insights on the same log lines.

| Metric (namespace `Konductor/EventStreams`) | Source | Value |
|---|---|---|
| `PublishAdmissionLatency` | Existing `AdmissionAccepted` log | `$.fields.latency_ms` |
| `PublishQueueWait` | New field on delivery's per-message `PublishDelivered` log | Invocation start minus `SentTimestamp` |
| `PublishEnqueueToDelivery` | Same log | EventBridge acceptance minus `SentTimestamp` |

Alarms:

- `PublishDlqDepthAlarm`: `ApproximateNumberOfMessagesVisible` of 1 or more, in the same shape as `SubscriptionsDlqDepthAlarm`.
- `PublishQueueAgeAlarm`: `ApproximateAgeOfOldestMessage` with the Maximum statistic above 300 s over a 300 s period.

The log lines carry no data, principal, or token fields.

### 9. Throttle budget check in pre-deployment validation

A new `throttle_budget` check in `validate-config.py` runs as part of normal validation. It counts `S = 2 × len(environments)` stages, where every stage has a publish limit (rate `R`, burst `B`) and a control-plane limit of 50 rps with a burst of 100.

- **Warning (validation still passes):** printed when `R + 50·S` exceeds the account rate quota `Q`, or `B + 100·S` exceeds the account burst `Q/2`. This is the case where one runaway stage can starve every other route. The warning names the publish throttle settings, the stage count, and both sides of the comparison.
- **Note (always printed):** the all-stages total, `(R + 50)·S` rps and `(B + 100)·S` burst, against `Q` and `Q/2`. It also says that other APIs in the account are not counted. With the defaults and three environments, the note reads 12,300 rps and 6,600 burst against 10,000 and 5,000.
- **Quota source:** the check follows the script's existing pattern of `aws` CLI calls with a fixture flag. It reads the applied quota with `aws service-quotas get-service-quota --service-code apigateway --quota-code L-8A5B8E43`, then falls back to `get-aws-default-service-quota`. If neither works (CLI missing, access denied, or `--skip-aws-checks`), it uses 10,000 and says so. A `--quota-fixture` flag supplies the value in tests, like `--ses-fixture` and `--stack-fixture`.
- **Burst estimate:** AWS does not expose the account burst as a quota and derives it from the rate. Both published default pairs (10,000/5,000 and 2,500/1,250) have a 2:1 ratio, so the check uses `Q/2` and says it is an estimate.
- **Output:** a new `warn()` helper prints `WARNING: …` to stderr. Warnings never change the exit code, so `make deploy-core` proceeds.

Alternatives considered:

- *Fail validation.* Rejected. An operator may set a high value on purpose, for example after raising the quota or when only one stage carries traffic.
- *Warn on the all-stages total.* Rejected. With the defaults it fires on every three-environment deployment, and a warning that always fires gets ignored.
- *Per-environment publish limits.* Deferred, as decided earlier. The check makes the shared-budget trade-off visible in the meantime.
### 10. Side-effects classification

**`publish-events` (delivery):**

- No DynamoDB write. The commits are now up to one `PutEvents` request per bus group, plus a `ChangeMessageVisibilityBatch` on messages that stay uncommitted.
- Rubric answers:
  1. Not applicable, because there is no DynamoDB write.
  2. Yes. Every `PutEvents` entry hangs on its inbound SQS command, and the visibility change only reschedules a command that did not commit.
  3. Yes. Entries carry the admission-fixed `idempotencyKey`. A crash after a partially accepted batch redelivers up to 10 messages instead of 1, which consumers dedupe as today.
  4. Yes, because delivery is asynchronous.
  5. Not applicable, because there is no stream.
  6. Yes, because the SQS command is the durable envelope.
- **Decision:** ENVELOPE-DRIVEN, unchanged. The wider duplicate window goes into the failure walk.

**`publish-events-admission`:**

- The only commit is still the one `SendMessage`. The pre-warm `GetItem` and `GetQueueAttributes` are read-only and come before any request.
- Rubric answers: (4) the path is synchronous and exempt, and (6) the queued command is the downstream envelope.
- **Decision:** STAY, unchanged.

Both records in `docs/development/side-effects/publishing.md` get dated updates.

## Risks / Trade-offs

- [Uncapped admission scale-out multiplies cold `DescribeSchema` calls, and a low Schemas quota answers `503`] → The optional cap, the `PublishAdmissionUnavailableAlarm` that already exists, and `service-quotas.md` guidance on sizing the cap from the Schemas rate. The schema read model change removes the registry from the hot path.
- [Uncapped admission can consume account concurrency that other functions need] → The account quota is raisable and documented as the main limit, and the cap is there when isolation matters.
- [Disable is no longer immediate, so publishes for up to one window (default 5 min) still deliver] → The window is configurable, and 0 restores per-request reads. The behavior change appears in the CHANGELOG, `domain-statuses.md`, `publish-validation.md`, and the publishing guide.
- [`maxReceiveCount` 25 makes non-throttle failures, such as a malformed or `INACTIVE` message, take about 12.5 min instead of about 90 s to reach the DLQ] → Accepted. Admission makes such messages rare, and the new queue-age and DLQ alarms make them visible.
- [Partial-batch crash widens the duplicate window from 1 to up to 10 events] → At-least-once delivery is already the contract, and consumers dedupe on `idempotencyKey`.
- [In-invocation retries lengthen invocations against a 30 s visibility timeout] → The retry budget stops 5 s before the invocation's deadline, and the remaining throttled entries move to the visibility backoff.
- [`ChangeMessageVisibility` fails] → It is logged, and the message falls back to the standard visibility timeout. No message is lost.
- [The publish throttle admits faster than the Region's `PutEvents` quota can deliver] → Admitted events queue up and drain at the EventBridge rate (`202` still means owned). The queue-age alarm signals a sustained deficit, and `service-quotas.md` explains how to size each limit.
- [The throttle budget check counts only Konductor's limits, but other APIs in the platform account share the same account throttle] → The note says so, and `service-quotas.md` tells operators to subtract other workloads' expected traffic when sizing the publish throttle.
- [The account burst is estimated as half the rate] → The estimate matches both published default pairs, and the warning labels it as an estimate. A wrong estimate only shifts when the warning fires, because the check never blocks a deployment.

## Migration Plan

1. **Deploy.** This is a normal `make deploy-core`. Validation now also prints the throttle budget note, and a warning when one stage could exceed the account throttle. Reading the quota needs `servicequotas:GetServiceQuota` and `servicequotas:GetAWSDefaultServiceQuota`. Without them, the check falls back to 10,000 and says so. Every new parameter has a default, and `make build` re-renders stages, so rendered stages pick up the publish method setting. CloudFormation removes admission's reserved concurrency, adds `ScalingConfig` to the event source mapping, and updates the queue's redrive policy in place. No resource is replaced.
2. **Compatibility.** The queue message format and the envelope do not change, so in-flight messages deliver identically before, during, and after the update.
3. **Validation.** Re-run the issue #6 load profile against a dev deployment. Compare median and p99 enqueue-to-delivery times, admission throttles, and DLQ arrivals.
4. **Rollback.** Redeploy the previous version. Messages with a receive count above 3 at rollback time go to the DLQ on their next receive, and are re-driven by re-publishing, as `PublishQueueAdmissionOnlyPolicy` requires.

## Open Questions

- Should the `PublishDeliveryMaxConcurrency` default of 50 and the 300 s queue-age threshold be adjusted after the benchmark re-run? Both are tunable without changing the specs.
