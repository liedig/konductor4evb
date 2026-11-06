# Tasks

## 1. Decision record and changelog

- [x] 1.1 Add `docs/decisions/tune-publish-path.md`. It records the publish method throttle and its 2,000 rps / 1,000 burst default against the shared account throttle, the throttle budget warning, the conditional admission cap, delivery `MaximumConcurrency` versus reserved concurrency, batched delivery, the throttle-recovery budget (`maxReceiveCount` 25), the ACTIVE-only liveness window, and the rejected alternatives from design.md. Use the existing record format: recorded date, change ID, area, status, Decision and rationale, Evidence. Index it under "Event contracts and delivery" in `docs/decisions/README.md`. Verify that the index link resolves with `mkdocs build --strict`.
- [x] 1.2 Add an `Unreleased` entry to `CHANGELOG.md`. It covers the dedicated publish throttle (default 2,000 rps with a burst of 1,000 per stage), the deploy-time throttle budget warning, the uncapped admission default and the optional cap, bounded delivery concurrency, batched delivery, throttle recovery, the five new settings, the new metrics and alarms, and the behavior change that disable now stops publishing within the domain liveness window (default 5 minutes). Verify that the entry sits under `## Unreleased` and names every new setting.

## 2. Shared library (`konductor-common`)

- [x] 2.1 Domain liveness cache. Write the failing tests first in the `#[cfg(test)]` module of a new `domain_liveness.rs`, and run them:
  - a fresh `ACTIVE` confirmation is served without calling `fetch`,
  - an expired confirmation calls `fetch` again (injected time source, no sleeping),
  - `INACTIVE` and missing results are never stored,
  - a zero window always fetches,
  - a `fetch` error propagates unchanged and stores nothing,
  - entries are isolated per domain,
  - the window comes from `DOMAIN_LIVENESS_CACHE_SECONDS`: unset or unparseable gives 300 s, and `0` disables caching.

  Then implement `DomainLivenessCache` with `///` docs and export it from `lib.rs`. Verify with `cargo test -p konductor-common domain_liveness`.
- [x] 2.2 Backoff and jitter. Write the failing tests first in a new `backoff.rs`, and run them:
  - the in-invocation retry delay uses full jitter from a 100 ms base with a 1 s cap,
  - the visibility backoff for receive count `n` lies within `[d/2, d]` for `d = min(60, 2^(n-1))` seconds, rounds up to whole seconds, and is at least 1 s,
  - the sum of the minimum visibility backoffs over receive counts 1-25 is at least 600 s, which ties the budget to the spec's 10-minute bar,
  - a deterministic `JitterSource` yields reproducible delays.

  Then implement `JitterSource`, its uuid-v4-backed default, and the two delay functions. Verify with `cargo test -p konductor-common backoff`.
- [x] 2.3 Request-size accounting. Write the failing tests first in `envelope.rs`:
  - entry size follows EventBridge's documented accounting and matches the bytes the existing per-entry size check measures, so admission and delivery agree,
  - a helper chunks prepared entries into requests of at most 10 entries and at most `PUT_EVENTS_ENTRY_MAX_BYTES` in total, keeping entry order,
  - a single entry at the size limit gets its own request.

  Then implement the helpers. Verify with `cargo test -p konductor-common envelope`.

## 3. Delivery (`publish-events`)

- [x] 3.1 Batched delivery. Write the failing `aws-smithy-mocks` tests first in the handler tests, and run them:
  - 10 same-bus records produce one `PutEvents` with 10 entries, each equal to the entry the record produces alone (source, detail-type, stamped version, idempotency key, `publishedAt`, metadata, data),
  - mixed legacy and format-2 records in one batch both deliver,
  - two buses produce two requests,
  - an oversized same-bus group is split by size,
  - a partial failure reports exactly the failed entries' message IDs, mapped by index,
  - a malformed record and an `INACTIVE`-domain record are reported without being sent while the rest deliver,
  - a whole-request non-throttle SDK error reports every message of that request.

  Then restructure `handle` into prepare, group, and concurrent send through `tokio::task::JoinSet`. Split the handler into role-named modules where that keeps files near the 300-line guideline, and update the `//!` module docs. Verify with `cargo test -p publish-events`.
- [x] 3.2 Delivery liveness cache. Write the failing tests first:
  - two records for the same domain in one batch make one domain `GetItem`,
  - a second invocation on the same warm cache within the window makes none,
  - an `INACTIVE` result is not reused, so the next batch reads again,
  - with a zero window, every batch reads once per domain,
  - the liveness read is strongly consistent, so a fresh confirmation cannot trail a disable.

  Then pass a `DomainLivenessCache` from `main.rs` into `handle`. Verify with `cargo test -p publish-events`.
- [x] 3.3 Throttle recovery. Write the failing tests first, using a deterministic jitter source:
  - an entry-level `ThrottlingException` accepted on the in-invocation retry is acknowledged, with no visibility change,
  - entries still throttled get one `ChangeMessageVisibilityBatch` on `PUBLISH_QUEUE_URL`, with each timeout derived from the record's `ApproximateReceiveCount`, and are reported in `batchItemFailures`; each visibility call is bounded by the invocation's remaining time,
  - a request-level `ThrottlingException` takes the same path,
  - a non-throttle entry error is reported without a visibility change,
  - a failed visibility call still reports the message,
  - no in-invocation retry starts when the remaining invocation time is under the 5 s margin,
  - an invocation with no time left skips the visibility change and still returns its response with the throttled message reported for retry.

  Then add `aws-sdk-sqs` to the crate, give `Clients` an SQS client, and implement the retry and backoff. Verify with `cargo test -p publish-events`.
- [x] 3.4 Latency fields. Write the failing tests first: the queue wait (invocation start minus `SentTimestamp`) and the enqueue-to-delivery time (EventBridge acceptance minus `SentTimestamp`) computed for a fixed `SentTimestamp` and clock. Then emit both as `queue_wait_ms` and `enqueue_to_delivery_ms` on the per-message `PublishDelivered` log line, with no data, principal, or token fields. The acceptance time is captured inside each `PutEvents` task, so a slow request never inflates another request's metric. Verify with `cargo test -p publish-events`, and check by reading the code that the field names match the metric filters in 5.1.
- [x] 3.5 Update the delivery documentation:
  - **`docs/architecture/publish-validation.md` §8:** batched `PutEvents` per bus, the liveness recheck reused within the window, throttle recovery with visibility backoff, and the larger receive budget.
  - **`docs/development/side-effects/publishing.md`:** a dated `publish-events` record for `tune-publish-path`, with the six rubric answers from design.md, the failure walk widened to a 10-message duplicate window, and the decision ENVELOPE-DRIVEN.
  - **`docs/development/side-effects/README.md`:** confirm the `publish-events` index row and add the dated update note.

  Verify with `mkdocs build --strict`.

## 4. Admission (`publish-events-admission`)

- [x] 4.1 Admission liveness cache. Write the failing handler tests first, and run them:
  - a second publish to the same domain on a warm cache makes no domain `GetItem`,
  - an `INACTIVE` result is not reused, so a publish after re-enable reads again and gets `202`,
  - with a zero window, every publish reads,
  - a cached `ACTIVE` confirmation with no publisher binding still returns `403`,
  - a domain-store failure with no reusable confirmation returns `503` and sends no message,
  - the liveness read is strongly consistent, so a fresh confirmation cannot trail a disable.

  Then thread a `DomainLivenessCache` through `handle` and `admit`, built in `main.rs`, ahead of the existing `require_active_domain`. Verify with `cargo test -p publish-events-admission`.
- [x] 4.2 Init pre-warm. Write the failing tests first for a `prewarm` function:
  - it issues one `GetItem` on the reserved non-domain key and one `GetQueueAttributes(QueueArn)` on the publish queue,
  - errors from either call are swallowed,
  - it returns within its timeout.

  Then call it from `main.rs` after building the clients, without any Schemas call. Verify with `cargo test -p publish-events-admission`.
- [x] 4.3 Update the admission and domain-status documentation:
  - **`docs/architecture/publish-validation.md` §2 and §4:** domain liveness is reused for at most the window and disable takes effect within it. Keep §6's registry-authority statement unchanged.
  - **`docs/reference/domain-statuses.md`:** update both `INACTIVE` rows.
  - **`docs/guides/publishing-events.md`:** in the error reference, publishes accepted within the window after a disable may still deliver.
  - **`docs/development/side-effects/publishing.md`:** a dated `publish-events-admission` record (STAY, pre-warm calls read-only).
  - **`docs/development/side-effects/README.md`:** confirm the admission index row.

  Verify with `mkdocs build --strict`.

## 5. Infrastructure and configuration

- [x] 5.1 Core stack. Write the failing template tests first in a new `scripts/tests/test_publish_path_template.py`, following the existing template-reading tests, and run them. The tests check that:
  - the five parameters exist with their defaults and bounds: `PublishThrottleRateLimit` 2000, `PublishThrottleBurstLimit` 1000, `PublishAdmissionConcurrencyCap` 0, `PublishDeliveryMaxConcurrency` 50 (2-1000), and `DomainLivenessCacheSeconds` 300 (0-900),
  - both authoring stages keep `/*` at 50 rps / 100 burst and add a `/~1publish` `POST` setting that references the throttle parameters,
  - admission `ReservedConcurrentExecutions` is the `HasPublishAdmissionConcurrencyCap` `!If` with `AWS::NoValue`,
  - `SqsTrigger` has `ScalingConfig.MaximumConcurrency` referencing its parameter,
  - `PublishQueue` has `maxReceiveCount` 25 and `VisibilityTimeout` 30,
  - delivery has `sqs:ChangeMessageVisibility` on `PublishQueue` only, and admission has `sqs:GetQueueAttributes`,
  - both functions carry `DOMAIN_LIVENESS_CACHE_SECONDS`, and delivery carries `PUBLISH_QUEUE_URL`,
  - the metric filters `PublishAdmissionLatency` (`AdmissionAccepted`, `$.fields.latency_ms`), `PublishQueueWait`, `PublishEnqueueToDelivery`, and `PublishDeliveryRetry` (count of messages reported for retry) exist,
  - the alarms `PublishDlqDepthAlarm`, `PublishQueueAgeAlarm` (300 s), and `PublishDeliveryRetryAlarm` (sustained retries over two 300 s periods) exist.

  Then edit `konductor-core-stack.yaml`, replacing the fixed-cap comment with the reason for the conditional cap. Verify with `uv run --with pytest --with pyyaml pytest scripts/tests/test_publish_path_template.py` and `sam validate --config-file samconfig-core.toml --lint`.
- [x] 5.2 Rendered stages. Write the failing renderer tests first in `scripts/tests/test_validate_config.py`, and run them:
  - every rendered stage on both APIs keeps `/*` at 50/100 and adds the `/~1publish` `POST` setting with `{ Ref: PublishThrottleRateLimit }` and `{ Ref: PublishThrottleBurstLimit }`,
  - the committed rendered block equals `render_stages_block` for the default environment list.

  Then update `render_stages_block` and regenerate the committed block with `make render-stages`. Verify with `uv run --with pytest --with pyyaml pytest scripts/tests/test_validate_config.py`.
- [x] 5.3 Settings. Write the failing tests first in `scripts/tests/test_validate_config.py`, and run them:
  - the five snake_case settings (`publish_throttle_rate_limit`, `publish_throttle_burst_limit`, `publish_admission_concurrency_cap`, `publish_delivery_max_concurrency`, `domain_liveness_cache_seconds`) are accepted and render into `--render` overrides under their parameter names as updateable settings,
  - non-integer values and booleans are rejected,
  - out-of-range values are rejected: rate < 1, burst < 0, cap < 0, delivery outside 2-1000, liveness outside 0-900.

  Then extend `SETTINGS`, type checking, and bounds in `scripts/validate-config.py`, and add commented defaults to `konductor-platform.toml.example`. Verify with `uv run --with pytest --with pyyaml pytest scripts/tests/test_validate_config.py`.
- [x] 5.4 Throttle budget warning. Write the failing tests first in `scripts/tests/test_validate_config.py`, and run them:
  - with the default throttle, three environments, and a 10,000 quota fixture, there is no warning, and the note reports 19,950 rps and a 21,900 burst (every stage's publish limit plus every stage's per-method control-plane ceiling) against 10,000 and 5,000,
  - with a 2,500 quota fixture, a rate warning names the publish throttle settings, the environment count, and 9,950 against 2,500,
  - a publish burst above half the quota gives a burst warning naming the burst setting alone (the control-plane burst ceiling already exceeds the estimate, so it is not counted there),
  - warnings leave the exit code at 0, and `--render` output is unchanged,
  - an applied quota is preferred over the AWS default quota,
  - a failed lookup, `--skip-aws-checks`, and an applied-quota read failure that falls back to the AWS default, each fall back with a note saying so.

  Then add the `warn()` helper, the `throttle_budget` check (`get-service-quota` for `L-8A5B8E43`, then `get-aws-default-service-quota`, then 10,000), and the `--quota-fixture` flag to `scripts/validate-config.py`. Verify with `uv run --with pytest --with pyyaml pytest scripts/tests/test_validate_config.py`.
- [x] 5.5 Update the configuration and quota documentation:
  - **`docs/reference/configuration.md`:** five settings rows and "Why these bounds" entries, including that a cap of 0 means uncapped and that a liveness window of 0 means a read on every request.
  - **`docs/operations/configure-platform.md`:** where to set them, what the throttle budget warning and note mean, and that the quota lookup needs `servicequotas:GetServiceQuota` and `servicequotas:GetAWSDefaultServiceQuota`.
  - **`docs/operations/service-quotas.md`:** the publish path's remaining limits (account Lambda concurrency, the account API Gateway throttle shared by every stage, and the `PutEvents` quota); the throttle budget math (one stage's publish limit plus every stage's control-plane limit, and the all-stages total, against the rate and the estimated half-rate burst), including subtracting other workloads in the account; sizing guidance for the publish throttle, delivery concurrency, and the admission cap against the Schemas rate; and the outdated "self-throttles the stage to 50 rps / 100 burst" row.
  - **`docs/guides/publishing-events.md`:** where `429` now comes from.
  - **`docs/reference/documentation-discrepancies.md`:** a D32 entry for the undercounted per-instance activation burst in `service-quotas.md`, reported rather than silently fixed.

  Verify with `mkdocs build --strict`.

## 6. Integration checks and review

- [x] 6.1 Run the full local gates and confirm they all pass: `make check`, `make test`, `uv run --with pytest --with pyyaml pytest scripts/tests`, `sam validate --config-file samconfig-core.toml --lint`, `mkdocs build --strict`, and `openspec validate tune-publish-path --strict`.
- [x] 6.2 Run the `rust-reviewer` droid on the Rust changes in `lib/konductor-common`, `publish-events`, and `publish-events-admission`. Resolve every finding, then re-run the gates in 6.1. There are no `web/` changes, so the `ui-reviewer` droid is not needed.

## Workflow follow-up

- Re-run the issue #6 load profile against a dev deployment. Record the median and p99 enqueue-to-delivery times, admission throttles, and DLQ arrivals in the decision record's Evidence. Revisit the `PublishDeliveryMaxConcurrency` default and the queue-age threshold (design.md Open Questions).
- Archive the change after review requirements are satisfied, then verify the archived specs.
