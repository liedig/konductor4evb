## MODIFIED Requirements

### Requirement: Subscription command queue and handler

Subscription write operations SHALL follow the established asynchronous write path with flat-verb routes: `POST /subscriptions` (create), `POST /subscriptions/state` (per-environment enable/disable), and `DELETE /subscriptions/{subscriptionId}` (unsubscribe). API Gateway SHALL enqueue the request onto the dedicated subscriptions command queue (with its own DLQ) carrying an `HttpMethod` message attribute, returning `202 Accepted` with the SQS message id as `requestId`. `consumerDomain` SHALL travel in the request body alongside the producer schema identity and per-environment directives; path-scoped domain routes SHALL NOT exist. The dedicated subscription command handler Lambda SHALL consume the queue, dispatch by route, and authorize through the grant engine BEFORE starting any workflow, using the normalized trusted principal propagated in the message envelope: the OAuth surface envelope SHALL carry the Cognito subject, team groups, and controlled global roles; the IAM surface envelope SHALL carry the authenticated AWS account and MAY carry the IAM principal ARN only as audit provenance. Structural validation SHALL occur before enqueueing, but the current binding-and-entitlement authorization decision SHALL occur only in the command handler, because a queued command may execute after revocation.

#### Scenario: Subscribe command enqueued and dispatched

- **WHEN** an authenticated caller sends `POST /subscriptions` passing structural validation
- **THEN** API Gateway SHALL enqueue the message on the subscriptions command queue and return `202 Accepted`
- **AND THEN** the subscription command handler SHALL authorize through the grant engine before dispatching it as a subscribe operation

#### Scenario: Authorization is current at execution time

- **WHEN** an entitlement is revoked after a command was enqueued but before the handler processes it
- **THEN** the handler's engine decision SHALL deny the command
- **AND THEN** no workflow SHALL start

#### Scenario: Read operations remain synchronous

- **WHEN** a caller sends `GET /subscriptions`, `GET /subscriptions/{subscriptionId}`, or a schema catalog request
- **THEN** API Gateway SHALL route directly to the dedicated read Lambda via `aws_proxy` integration with no queue involved

#### Scenario: Failed subscription commands captured in DLQ

- **WHEN** the subscription command handler exhausts SQS retries on a message
- **THEN** the message SHALL move to the subscriptions command DLQ with at least 14 days retention for operator inspection and replay

### Requirement: Subscription command outcome events

The subscription command handler and workflows SHALL emit outcome events on the control-plane bus following the established `{Operation}Succeeded|Failed` convention: `SubscribeSucceeded|SubscribeFailed`, `UpdateSubscriptionSucceeded|UpdateSubscriptionFailed`, and `UnsubscribeSucceeded|UnsubscribeFailed`. Validation and authorization failures (unknown or inactive domain, missing subscriber binding, missing covering entitlement, rule-name constraint violations) SHALL emit the `Failed` outcome with the error reason rather than dead-lettering the message. Failure reasons SHALL NOT include tokens, claims material, or schema payload content.

#### Scenario: Authorization failure emits outcome event

- **WHEN** a subscribe command is denied because the caller lacks a subscriber binding or no current entitlement covers the target producer schema
- **THEN** the handler SHALL emit `SubscribeFailed` with the authorization reason
- **AND THEN** the message SHALL be acknowledged (not retried) since the failure is deterministic

#### Scenario: Validation failure emits outcome event

- **WHEN** a subscribe command fails business validation (e.g., unknown domain or rule-name constraint violation)
- **THEN** the handler SHALL emit `SubscribeFailed` with the validation error
- **AND THEN** the message SHALL be acknowledged (not retried) since the failure is deterministic
