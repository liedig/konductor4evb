# Spec Delta

## ADDED Requirements

### Requirement: Dedicated publish throttle

`POST /publish` on every environment stage of both control-plane surfaces SHALL have its own API Gateway throttle, independent of other routes. Its rate and burst SHALL be operator settings that default to 2,000 requests per second with a burst of 1,000 per stage. Every other route SHALL keep its existing stage throttle. A request over the publish throttle SHALL receive `429 Too Many Requests` and SHALL NOT be queued.

#### Scenario: Publish traffic does not consume the control-plane throttle

- **WHEN** publish traffic on a stage exceeds the control-plane routes' stage throttle
- **THEN** publish requests SHALL be throttled only against the publish throttle
- **AND THEN** control-plane routes on that stage SHALL keep their own stage throttle, which publish requests do not count against

#### Scenario: Default publish throttle

- **WHEN** the platform is deployed without publish throttle settings
- **THEN** `POST /publish` on every environment stage of both the OAuth and IAM surfaces SHALL allow 2,000 requests per second with a burst of 1,000

#### Scenario: Throttled publish is not accepted

- **WHEN** a publish request exceeds the publish throttle
- **THEN** the API SHALL return `429 Too Many Requests`
- **AND THEN** no SQS message SHALL be queued, and the publisher SHALL own the event

### Requirement: Admission concurrency is uncapped by default

Publish admission SHALL NOT reserve or cap Lambda concurrency unless the operator configures an admission concurrency cap. A configured cap SHALL apply as the admission function's reserved concurrency. A request rejected by Lambda throttling SHALL NOT be queued, and the publisher SHALL own the event.

#### Scenario: Default deployment does not cap admission

- **WHEN** the platform is deployed without an admission concurrency cap
- **THEN** publish admission SHALL scale with the account's available Lambda concurrency

#### Scenario: Operator-configured cap

- **WHEN** the operator configures an admission concurrency cap of N
- **THEN** publish admission SHALL run at most N concurrent invocations
- **AND THEN** requests beyond the cap SHALL fail without being queued

### Requirement: Bounded delivery concurrency

The publish delivery consumer SHALL process the publish queue with at most the operator-configured number of concurrent invocations, defaulting to 50. When delivery is saturated, admitted events SHALL wait in the queue rather than fail.

#### Scenario: Saturated delivery builds a backlog, not failures

- **WHEN** admitted events arrive faster than the maximum delivery concurrency can deliver them
- **THEN** delivery SHALL NOT exceed the configured concurrency
- **AND THEN** the undelivered events SHALL remain queued and SHALL be delivered as capacity frees

#### Scenario: Default delivery concurrency

- **WHEN** the platform is deployed without a delivery concurrency setting
- **THEN** the publish delivery consumer SHALL run at most 50 concurrent invocations

### Requirement: Batched delivery to domain buses

Delivery SHALL send admitted events bound for the same event bus in shared `PutEvents` requests, each within EventBridge's per-request entry-count and size limits, and SHALL send requests for different buses independently. Each entry SHALL equal the entry the event would produce if delivered alone. Only messages whose entries are not accepted SHALL be reported for retry.

#### Scenario: Same-bus batch shares one request

- **WHEN** a delivery batch holds 10 admitted events for the same bus whose entries fit one request
- **THEN** delivery SHALL make one `PutEvents` request with 10 entries
- **AND THEN** each entry SHALL carry its own message's source, detail-type, stamped schema version, idempotency key, `publishedAt`, metadata, and data

#### Scenario: Oversized group is split

- **WHEN** the entries for one bus together exceed the `PutEvents` per-request size limit
- **THEN** delivery SHALL split them across requests so that no request exceeds the entry-count or size limit

#### Scenario: Partial PutEvents failure

- **WHEN** `PutEvents` accepts some entries of a request and rejects others
- **THEN** only the messages of the rejected entries SHALL be reported for retry
- **AND THEN** the messages of the accepted entries SHALL be acknowledged

#### Scenario: Per-message failures stay isolated

- **WHEN** a delivery batch contains a malformed message or a message for a domain that is not `ACTIVE`
- **THEN** that message SHALL be reported for retry without being sent to EventBridge
- **AND THEN** the batch's other messages SHALL be delivered

### Requirement: Throttled delivery recovery

When EventBridge throttles delivery, the consumer SHALL retry the throttled entries within the same invocation with jittered backoff. A message still throttled SHALL become visible for redelivery after a jittered exponential backoff capped at 60 seconds, not the full visibility timeout. The publish queue's receive budget SHALL tolerate at least 10 minutes of sustained throttling before an event is dead-lettered.

#### Scenario: Transient throttling is absorbed in the invocation

- **WHEN** `PutEvents` throttles some entries and the in-invocation retry accepts them
- **THEN** their messages SHALL be acknowledged and none SHALL be reported for retry

#### Scenario: Persistent throttling shortens redelivery

- **WHEN** entries remain throttled after the in-invocation retries
- **THEN** their messages SHALL be reported for retry
- **AND THEN** each SHALL become visible again after a jittered backoff that grows with its receive count and does not exceed 60 seconds

#### Scenario: An out-of-time invocation keeps its response

- **WHEN** throttled entries remain and the invocation has no time left for the visibility change
- **THEN** their messages SHALL keep the queue's standard visibility timeout
- **AND THEN** the invocation SHALL still return its response, acknowledging every accepted entry and reporting the throttled messages for retry

#### Scenario: Sustained throttling does not dead-letter valid events

- **WHEN** EventBridge throttles delivery continuously for 10 minutes
- **THEN** no admitted event SHALL reach the publish DLQ because of throttling alone

#### Scenario: Other failures keep standard retry timing

- **WHEN** an entry fails for a reason other than throttling, or a message fails before `PutEvents`
- **THEN** the message SHALL be reported for retry with the queue's standard visibility timeout

### Requirement: Bounded domain liveness caching on the publish path

Admission and delivery SHALL each reuse a confirmation that a domain is `ACTIVE` for no longer than the configured domain liveness window, defaulting to 300 seconds; a window of 0 SHALL disable reuse. `INACTIVE` and missing results SHALL NOT be reused. For a domain disabled at time t0, no event SHALL be admitted or delivered after t0 plus the window.

#### Scenario: Cached confirmation avoids a read

- **WHEN** a warm admission or delivery instance confirmed a domain `ACTIVE` within the window
- **THEN** it SHALL check that domain's liveness without reading the domain store

#### Scenario: Disable takes effect within the window

- **WHEN** a platform admin disables a domain at time t0
- **THEN** every publish request admitted after t0 plus the window SHALL instead receive `404 Not Found`
- **AND THEN** every message for that domain processed by delivery after t0 plus the window SHALL be reported for retry without being sent, following the existing retry and DLQ handling

#### Scenario: Negative results are never reused

- **WHEN** a publish observed a domain as `INACTIVE` or missing and the domain then becomes `ACTIVE`
- **THEN** the next publish for that domain SHALL read the domain store and SHALL be admitted when otherwise valid

#### Scenario: Zero window reads every time

- **WHEN** the domain liveness window is configured as 0
- **THEN** every admission SHALL read the domain store
- **AND THEN** every delivery batch SHALL read the domain store for each domain its messages target before sending them

#### Scenario: Cached liveness does not bypass authorization

- **WHEN** a caller without a `publisher` binding publishes to a domain whose `ACTIVE` confirmation is cached
- **THEN** admission SHALL return `403 Forbidden`, exactly as without the cache

#### Scenario: Required read fails

- **WHEN** admission needs a domain read because no confirmation is reusable, and the domain store fails
- **THEN** the API SHALL return `503 Service Unavailable` and SHALL NOT queue the event

### Requirement: Publish latency observability

The platform SHALL publish metrics for admission latency, the time an admitted event waits in the publish queue, and the time from queue acceptance to EventBridge acceptance. It SHALL alarm when the publish DLQ holds messages, when the oldest queued publish message exceeds an age threshold, and when delivery keeps reporting publish messages for retry. These metrics SHALL NOT carry payload data, principals, or tokens.

#### Scenario: Latency metrics are emitted

- **WHEN** an event is admitted and later accepted by EventBridge
- **THEN** the platform SHALL record its admission latency, its queue wait, and its queue-to-EventBridge latency as metrics

#### Scenario: Latency metrics measure each event's own request

- **WHEN** concurrent `PutEvents` requests deliver a batch and one request completes far later than another
- **THEN** the queue-to-EventBridge latency recorded for an event SHALL be measured by the request that accepted it, not by the slowest request of the batch

#### Scenario: Dead-lettered publish raises an alarm

- **WHEN** a message reaches the publish DLQ
- **THEN** the publish DLQ depth alarm SHALL enter the alarm state

#### Scenario: Aging backlog raises an alarm

- **WHEN** the oldest message in the publish queue exceeds the age threshold
- **THEN** the publish queue age alarm SHALL enter the alarm state

#### Scenario: Sustained delivery retries raise an alarm

- **WHEN** delivery reports publish messages for retry continuously for ten minutes — for example sustained EventBridge throttling, an inactive domain, or poison messages — while the queue age metric cannot see repeatedly received messages on a standard queue
- **THEN** the publish delivery retry alarm SHALL enter the alarm state
