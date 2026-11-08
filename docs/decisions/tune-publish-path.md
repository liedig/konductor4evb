# Tune the publish path: dedicated throttle, batching, throttle recovery, and liveness caching

[Documentation home](../README.md)

- Recorded date: 2026-11-02
- Change ID: `tune-publish-path`
- Status: recorded decision; code, template, and live re-benchmark evidence named below.

## Context

[Issue #6](https://github.com/liedig/konductor4evb/issues/6) benchmarked the
publish path at 1,000 messages per second after warm-up. It measured a median
end-to-end publish time of about 165 ms (p99 about 296 ms) against about
75 ms for native EventBridge publishing. During warm-up, 294 requests were
throttled at the admission reserved-concurrency cap of 100, and the queued
path hit EventBridge throttling with queue-wait spikes of up to 150 seconds.

Reading the code showed the platform set its own limits below the AWS quotas:
one `PutEvents` call per record in delivery, an uncached domain `GetItem` per
event in both admission and delivery, a single stage-wide API Gateway throttle
of 50 rps / 100 burst shared by `/publish` and every control-plane route, a
fixed admission cap of 100, no delivery concurrency bound, and throttled
messages waiting out the full 30-second visibility timeout with a
`maxReceiveCount` of 3 — enough for sustained throttling to dead-letter valid
events.

## Decision and rationale

Five levers, all operator-tunable through the
[platform configuration](../reference/configuration.md):

1. **Dedicated publish throttle.** `POST /publish` gets a method-level
   throttle on every environment stage of both APIs
   (`publish_throttle_rate_limit` 2,000 rps, `publish_throttle_burst_limit`
   1,000, per stage). Method-level settings override the stage-wide entry for
   that method only, so every other route keeps 50 rps / 100 burst as a
   backstop. Stage and method throttles are ceilings against the account's
   shared, per-Region API Gateway throttle (10,000 rps / 5,000 burst by
   default, adjustable), not reservations. *Rejected alternatives:* literal
   rendered values (a setting change would then need a re-render), usage
   plans (require API keys, do not fit OAuth/IAM callers), and account
   defaults as the per-stage value (one stage could starve every other
   route).
2. **Deploy-time throttle budget warning.** `make validate-config` compares
   the configured limits against the account API Gateway throttle quota
   (`L-8A5B8E43`, read through Service Quotas with the documented 10,000 rps
   fallback). It warns — never fails — when one stage at its publish limit
   plus every stage's control-plane ceiling exceeds the account rate. API
   Gateway applies the stage-wide `/*` throttle independently to every
   method, so the control-plane ceiling is the method count (53 per
   environment, counted from the two OpenAPI contracts at validation time) times
   50 rps, not one shared 50-rps bucket per stage. The burst warning isolates
   the publish setting (`B > Q/2`), because the control-plane burst ceiling
   already exceeds the estimated account burst for even one environment and
   counting it would fire on every deployment. The check always notes the
   all-stages total, and it discloses which quota source it used — the
   applied quota, the AWS default, or the documented 10,000 rps fallback.
   *Rejected alternatives:* failing validation (an operator may set a high
   value on purpose after raising the quota), and a burst warning that
   counts the control-plane ceiling (always fires; a warning that always
   fires gets ignored).
3. **Domain liveness cache.** Admission and delivery share an ACTIVE-only
   cache (`domain_liveness_cache_seconds`, default 300, range 0–900, 0
   disables) held per warm Lambda instance. Only ACTIVE confirmations are
   cached; INACTIVE and missing domains are always re-read, so re-enabling a
   domain works on the next publish. The reads are strongly consistent, so a
   fresh confirmation cannot trail a disable that has just happened and the
   bounded-disable contract holds from the read itself. A confirmation's
   window starts when its read begins, so a slow read cannot extend it, and
   a read that takes the whole window or longer confirms nothing (admission
   answers `503`, delivery reports the message for retry), because its
   `ACTIVE` answer may already trail a disable. Admission re-checks before enqueueing and delivery re-confirms each
   chunk's domains immediately before its `PutEvents` request is
   dispatched; each check reads the store only for a confirmation that
   expired while the request was validated, the batch was prepared, another
   domain was checked, or a throttle backoff ran. With a zero window every
   request and batch already reads at its start, so only resends re-read.
   Both admission's `SendMessage` and delivery's `PutEvents` run with
   SDK-level retries disabled (one attempt per call), because a delayed
   automatic retry would skip the liveness re-check; transient failures
   surface to the caller (admission 503) or the retry loop (delivery), both
   of which re-check. Store errors
   propagate and are never cached. A cached confirmation never bypasses
   authorization, which runs first. Admission also pre-warms its DynamoDB
   and SQS connections during Lambda init with best-effort calls (2-second
   total timeout). *Rejected alternatives:* a distributed cache or
   stream-driven invalidation (execution environments are not addressable),
   caching negative results (re-enable must take effect immediately).
4. **Batched delivery with per-entry failure mapping.** Delivery groups a
   batch by target bus into `PutEvents` requests of at most 10 entries and
   256 KB, sends them concurrently, and maps each failed entry back to its
   SQS message by index (Accepted / Throttled / Failed). Delivery concurrency
   is bounded by the event source mapping's `MaximumConcurrency`
   (`publish_delivery_max_concurrency`, default 50, range 2–1000) rather than
   reserved concurrency, so a saturated consumer leaves messages queued
   instead of raising their receive counts. *Rejected alternatives:* one
   request per record sent concurrently (keeps 10× the `PutEvents` calls),
   reserved concurrency on delivery (the poller keeps receiving and Lambda
   throttles, which is the DLQ path this change exists to close).
5. **Throttle recovery.** Throttled entries are retried inside the
   invocation with full-jitter backoff (100 ms base, 1 s cap, up to 3
   retries, skipped when under 5 seconds remain before the deadline).
   Preparation runs under the same governance: a record's parse, liveness
   read, and envelope build starts only while more than 3 seconds remain
   (the response margin plus a 1-second send floor); a record that misses
   the floor is reported for retry unprepared so slow store reads cannot
   starve the send rounds of the entries already prepared. Every
   send round, retries included, is bounded by the remaining time minus a
   2-second response margin; a round that runs out reports its messages for
   retry, so slow `PutEvents` calls cannot time out the invocation and lose
   the acknowledgements of entries already accepted. Messages still throttled get `ChangeMessageVisibilityBatch` with backoff
   `d/2 + jitter(0, d/2)` where `d = min(60, 2^(n-1))` for receive count
   `n`, so retries spread out instead of waiting the full 30-second
   visibility timeout. Each visibility call is bounded by the invocation's
   remaining time minus the same 2-second response margin and skipped when
   none remains, so the best-effort change can never cost the batch its
   response or the acknowledgements of already accepted entries; the
   standard timeout is the fallback. The publish
   queue's `maxReceiveCount` rises from 3 to
   25, so even minimum-backoff redrives keep a message alive for over 600
   seconds of sustained throttling before the DLQ. *Rejected alternatives:*
   the visibility timeout alone with a larger receive budget (slow recovery,
   synchronized retry herd), and a separate retry queue (another queue to
   secure; re-sending would replace the `SentTimestamp` that sets
   `publishedAt`).

Latency observability comes with the change: the `PublishDelivered` log gains
`queue_wait_ms` and `enqueue_to_delivery_ms` — each captured inside its own
`PutEvents` task, so a slow request cannot inflate a fast one's latency —
surfaced as the `PublishQueueWait` and `PublishEnqueueToDelivery` metrics,
and new alarms fire on publish DLQ depth, on the oldest queued message
exceeding 300 seconds, and on delivery reporting messages for retry
continuously for ten minutes (the `PublishDeliveryRetry` metric, one data
point per message reported for retry — batch-level diagnostics carry no
message_id and stay out of the count; SQS stops
counting repeatedly received messages in the age metric after three
receives, so the retry alarm covers the window between the age alarm's
blind spot and the DLQ).

## Trade-offs

- **Wider duplicate window.** The path stays at-least-once. A crash after a
  partially accepted batch now redelivers up to 10 messages instead of 1;
  consumers dedupe on `idempotencyKey` as before.
- **Disable is no longer immediate.** Disabling a domain stops admission and
  delivery within the liveness window (default 5 minutes), not on the next
  request. The window is configurable and 0 restores per-request reads.
- **Admission reserved concurrency is conditional.** It is omitted by
  default (cap 0), so admission draws from the account's shared pool. This
  removes the queue-starvation coupling between the reserved admission
  capacity and the delivery poller, but an uncapped scale-out multiplies
  cold `DescribeSchema` calls against the Schemas quota; the optional cap
  mitigates until a schema read model takes the registry off the publish
  path.
- **Non-throttle failures wait longer.** With `maxReceiveCount` 25, a
  malformed message takes about 12.5 minutes instead of about 90 seconds to
  reach the DLQ; the queue-age, delivery-retry, and DLQ alarms make this
  visible.

## Evidence

- `cargo test -p konductor-common` (liveness cache, backoff budget, entry
  chunking), `cargo test -p publish-events`, and
  `cargo test -p publish-events-admission` cover every behavior above with
  `aws-smithy-mocks` and deterministic jitter.
- Template and validator checks:
  `scripts/tests/test_publish_path_template.py` and
  `scripts/tests/test_validate_config.py`.
- The issue #6 load profile was re-run against a dev deployment of this
  change (domain `sales`, ap-southeast-2, runs `bench-20261102-134320`
  and `bench-20261102-135719`; harness:
  `konductor-benchmarks`, in-region Lambda driver, 1,000 events/s for
  450s, 200 in flight, plus a 100 events/s native `PutEvents` floor on a
  throwaway bus):
  - Publish API: 420,000 measured publishes, 100% accepted, **zero
    throttles** (issue #6 saw 294 at the same profile), p50 31.2 ms,
    p95 42.4 ms, p99 67.9 ms — down from p50 164.7 ms / p99 295.7 ms on
    the inline path.
  - Stage breakdown: admission p50 12.1 ms / p99 37.9 ms, queue wait
    p50 38.1 ms / p99 94.2 ms, enqueue-to-delivery p50 50.5 ms /
    p99 133.6 ms. The queue-wait p99 of ~94 ms against the 300-second
    queue-age alarm threshold confirms the threshold and the default
    delivery concurrency both hold at this rate; no retuning needed.
  - Native floor: p50 2.9 ms / p99 18.9 ms, so the tuned platform sits
    ~10x above raw `PutEvents` at the median — the admission +
    enqueue cost, with delivery no longer in the request path.
  - Reconciliation: 450,000 accepted publishes (warm-up included) =
    450,000 arrivals, DLQ depth delta 0, `matched: true` — no loss at
    1,000 events/s sustained.
  - Arrival-latency p95/p99 via the CloudWatch Logs probe read ~49s/59s;
    that path is batched log delivery, not bus delivery, so the
    enqueue-to-delivery stage metric is the authoritative figure.

[Decision index](README.md)
