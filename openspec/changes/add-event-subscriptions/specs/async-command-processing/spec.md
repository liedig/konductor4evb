# Capability: Async Command Processing (delta)

## ADDED Requirements

### Requirement: Subscription command queue and handler

Subscription write operations (`POST /subscriptions`, `PATCH /subscriptions/{subscriptionId}`, `DELETE /subscriptions/{subscriptionId}`) SHALL follow the established asynchronous write path: API Gateway SHALL enqueue the request onto a dedicated subscriptions command queue (with its own DLQ) carrying an `HttpMethod` message attribute and path parameters injected into the body, returning `202 Accepted` with the SQS message id as `requestId`. A dedicated subscription command handler Lambda SHALL consume the queue, dispatch by `HttpMethod` (POST → subscribe, PATCH → update per-environment state, DELETE → unsubscribe), perform business validation, and start the appropriate workflow. The caller's AWS account identity from the API Gateway IAM authorizer SHALL be propagated in the message for ownership validation.

#### Scenario: Subscribe command enqueued and dispatched

- **WHEN** an authenticated caller sends `POST /subscriptions` passing structural validation
- **THEN** API Gateway SHALL enqueue the message on the subscriptions command queue and return `202 Accepted`
- **AND THEN** the subscription command handler SHALL dispatch it as a subscribe operation carrying the caller's account identity

#### Scenario: Read operations remain synchronous

- **WHEN** a caller sends `GET /subscriptions`, `GET /subscriptions/{subscriptionId}`, or a schema discovery request
- **THEN** API Gateway SHALL route directly to the dedicated read Lambda via `aws_proxy` integration with no queue involved

#### Scenario: Failed subscription commands captured in DLQ

- **WHEN** the subscription command handler exhausts SQS retries on a message
- **THEN** the message SHALL move to the subscriptions command DLQ with at least 14 days retention for operator inspection and replay

### Requirement: Subscription command outcome events

The subscription command handler and workflows SHALL emit outcome events on the control-plane bus following the established `{Operation}Succeeded|Failed` convention: `SubscribeSucceeded|SubscribeFailed`, `UpdateSubscriptionSucceeded|UpdateSubscriptionFailed`, and `UnsubscribeSucceeded|UnsubscribeFailed`. Validation failures (unknown domain, ownership mismatch, rule-name constraint violations) SHALL emit the `Failed` outcome with the error reason rather than dead-lettering the message.

#### Scenario: Validation failure emits outcome event

- **WHEN** a subscribe command fails business validation (e.g., caller does not own the consumer domain)
- **THEN** the handler SHALL emit `SubscribeFailed` with the validation error
- **AND THEN** the message SHALL be acknowledged (not retried) since the failure is deterministic
