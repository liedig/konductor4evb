# Proposal

## Why

Issue #6 benchmarked the publish path at 1,000 messages per second after warm-up. It measured a median delivery time of 164.7 ms, against 74.7 ms for native EventBridge publishing, and a version without the queue cut Konductor's overhead from about 90 ms to about 25 ms. During warm-up, 294 requests were throttled at the admission cap of 100 concurrent executions, and the queued path hit EventBridge throttling with delays of up to 150 seconds.

Reading the code shows where the time and the limits come from:

- **Delivery runs one record at a time.** `publish-events` handles a 10-record batch serially, with one `PutEvents` call per record.
- **The domain is read twice per event.** Admission and delivery each make an uncached `GetItem` on the domain.
- **Konductor sets its own low limits.** The API Gateway stage throttle (50 rps, burst 100) covers `/publish` together with every control-plane route. Admission also has a hard reserved-concurrency cap of 100.
- **Delivery has no concurrency bound.** Nothing stops it from scaling into EventBridge throttling.
- **Throttled records wait too long and can be lost to the DLQ.** They wait out the full 30-second visibility timeout, and three receives are enough to dead-letter a valid event.

The publish API should perform as well as the platform allows. Its limits should be AWS quotas that operators can raise, or Konductor settings that operators can tune. Those settings should be sized so that publish traffic cannot use up the API Gateway throttle that the account shares with every other route. The change keeps today's store-and-forward contract: after a `202`, Konductor owns delivery.

## What Changes

- **Separate publish throttle.** `POST /publish` gets its own API Gateway method throttle on every environment stage of both the OAuth and IAM surfaces, rendered by `scripts/validate-config.py`. Its rate and burst come from new settings that default to 2,000 rps with a burst of 1,000 per stage. With these defaults, one stage at its full publish rate plus every stage's control-plane routes stays well inside the default account throttle (10,000 rps, burst 5,000), which every API and stage in the account shares. Every other route keeps 50 rps and a burst of 100.
- **Throttle budget warning.** Pre-deployment validation compares the configured API Gateway limits with the account's API Gateway throttle quota in the deployment Region. It warns, without failing, when one stage's publish limit plus every stage's control-plane limit exceeds the account rate or burst. It always reports the total for all stages as a note.
- **No admission concurrency cap by default.** `publish-events-admission` loses its fixed reserved concurrency. An optional setting restores a cap.
- **Bounded delivery concurrency.** The publish queue's event source mapping gets a configurable `MaximumConcurrency` (default 50), so EventBridge throttling builds a backlog in the queue instead of an invocation storm.
- **Batched delivery.** `publish-events` groups a batch by target bus into `PutEvents` requests within EventBridge's entry-count and request-size limits, sends the groups concurrently, and maps each failed entry back to its SQS message. The delivered envelope does not change.
- **Throttle recovery.**
  - Throttled entries are retried inside the invocation with backoff and jitter.
  - Messages still throttled after that become visible again after a short exponential backoff with jitter, instead of the full visibility timeout.
  - The queue's receive budget rises so that sustained throttling does not send valid events to the DLQ.
  - Failures for other reasons keep today's timing.
- **Domain liveness cache.** Admission and delivery share a cache in `konductor-common` that remembers only `ACTIVE` domains. Its lifetime is configurable, defaults to 5 minutes, and a value of 0 turns caching off. `INACTIVE` and missing domains are never cached. **Behavior change:** disabling a domain now stops publishing and delivery within the configured lifetime, instead of on the next request. Re-enabling still takes effect immediately.
- **Init pre-warm.** Admission opens its DynamoDB and SQS connections during Lambda init with best-effort calls, so the first request on a new instance doesn't pay for the TLS handshakes.
- **Latency observability.** New metrics for admission latency, queue wait, and enqueue-to-delivery time, plus alarms on publish DLQ depth and on the age of the oldest queued message.
- **Documentation.**
  - A decision record.
  - Updates to the publish-validation architecture page, the service-quotas page (which limits remain and how to size them), the configuration reference, the publishing guide, and the side-effects records.
  - A discrepancy entry for the undercounted activation burst in `service-quotas.md`.
  - A `CHANGELOG.md` entry.

## Capabilities

### New Capabilities

None. The behavior belongs to the existing event-publishing and domain-management capabilities.

### Modified Capabilities

- `event-publishing`: adds requirements for:
  - publish-path capacity limits (a separate publish throttle, no admission cap by default, bounded delivery concurrency),
  - batched delivery,
  - throttled-delivery recovery,
  - bounded domain liveness caching,
  - latency observability.
- `domain-management`: platform-managed disable now stops publishing and delivery within the configured liveness window instead of immediately. Re-enable and subscription-activation behavior are unchanged.
- `system-architecture`:
  - the platform configuration contract gains the five publish-path settings as updateable settings,
  - pre-deployment validation range-checks them,
  - a new requirement adds the throttle budget warning.

## Impact

- **Code:** `event-streams/functions/publish-events` (batching, throttle recovery, domain cache, latency fields), `event-streams/functions/publish-events-admission` (domain cache, init pre-warm), and a new shared liveness cache in `lib/konductor-common`.
- **Infrastructure:** `konductor-core-stack.yaml`:
  - new parameters,
  - publish method throttles on the authoring stages,
  - conditional admission reserved concurrency,
  - delivery `ScalingConfig`,
  - the publish queue's `maxReceiveCount`,
  - IAM changes: delivery gets `sqs:ChangeMessageVisibility`, admission gets `sqs:GetQueueAttributes`,
  - environment variables, metric filters, and alarms.
- **Configuration:** `scripts/validate-config.py`, `konductor-platform.toml.example`, and `docs/reference/configuration.md` gain five updateable settings. The rendered environment stages carry the publish throttle. `validate-config.py` reads the API Gateway account throttle quota (`L-8A5B8E43`) through the Service Quotas API with the operator's credentials. When that read is not possible, it falls back to the documented default and says so.
- **APIs:** request and response contracts are unchanged. `429` responses from API Gateway now start at the publish throttle rather than the shared 50 rps stage limit.
- **Operations:** the publish throttle setting, the account Lambda concurrency, the API Gateway account throttle, and the EventBridge `PutEvents` quota become the publish path's main limits.
  - Without a cap, admission can scale far beyond 100 instances. Each new instance fetches its schemas from the registry, so with a low Schemas API quota a sharp scale-out can trigger fail-closed `503`s. The optional cap mitigates this until a schema read model takes the registry off the publish path.
- **Out of scope** (separate changes or deferred):
  - rates per environment or per domain,
  - the schema read model,
  - copies of the metadata pointer and bindings to remove hot-item limits,
  - caching Allow decisions,
  - direct publishing with a queue fallback,
  - changing Lambda memory size.
