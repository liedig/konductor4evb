## MODIFIED Requirements

### Requirement: Subscription command queue and handler

Subscription write operations (`POST /domains/{consumerDomain}/subscriptions`, `PATCH /domains/{consumerDomain}/subscriptions/{subscriptionId}`, and `DELETE /domains/{consumerDomain}/subscriptions/{subscriptionId}`) SHALL be placed on both OAuth and IAM API surfaces and SHALL use the established asynchronous write path. API Gateway SHALL enqueue each structurally valid request directly onto the dedicated subscriptions command queue through an AWS service integration, without an intervening Lambda. The queue SHALL have its own DLQ. The message SHALL carry an `HttpMethod` attribute, path parameters injected into the body, and the common trusted-principal envelope: OAuth commands SHALL preserve the Cognito subject, current team groups, and `platform-admin` or `platform-auditor` roles, while IAM commands SHALL preserve the AWS account and IAM principal identity. API Gateway SHALL return `202 Accepted` with the SQS message id as `requestId` without waiting for authorization or business processing.

A dedicated subscription command handler Lambda SHALL consume the queue, dispatch by `HttpMethod` and route context (`POST` to create, `PATCH` to update per-environment state, and `DELETE` to delete), perform business and authorization validation against current role bindings and entitlements through the common decision contract, and start the appropriate workflow. OAuth decisions SHALL use Cognito team-group bindings and IAM decisions SHALL use AWS-account bindings at account granularity. The handler SHALL derive the consumer domain from the trusted path parameter and SHALL NOT accept it from a mutable request body.

#### Scenario: Create command directly enqueued and dispatched

- **WHEN** an authenticated caller sends a structurally valid `POST /domains/checkout/subscriptions`
- **THEN** API Gateway SHALL enqueue the command directly on the subscriptions command queue and return `202 Accepted`
- **AND THEN** the subscription command handler SHALL dispatch it as a create operation for consumer domain `checkout` with trusted caller authorization context

#### Scenario: Update and delete paths supply ownership scope

- **WHEN** API Gateway enqueues a structurally valid subscription `PATCH` or `DELETE`
- **THEN** it SHALL inject both `consumerDomain` and `subscriptionId` path parameters into the command envelope
- **AND THEN** the handler SHALL verify that the subscription is owned by that consumer domain

#### Scenario: IAM command preserves account-granularity authorization

- **WHEN** an AWS_IAM-authenticated caller sends a structurally valid subscription command on the IAM surface
- **THEN** API Gateway SHALL enqueue the trusted AWS account and IAM principal identity
- **AND THEN** the command handler SHALL authorize the domain role from the AWS-account binding rather than the individual IAM role or session name

#### Scenario: Subscription and catalog reads remain synchronous

- **WHEN** a caller sends `GET /domains/{domain}/subscriptions`, `GET /subscriptions/{subscriptionId}`, or a consumer-domain catalog request on either resource API surface
- **THEN** API Gateway SHALL route directly to the appropriate read Lambda through `aws_proxy` integration with no queue involved

#### Scenario: Failed subscription commands captured in DLQ

- **WHEN** the subscription command handler exhausts SQS retries on a message
- **THEN** the message SHALL move to the subscriptions command DLQ with at least 14 days retention for operator inspection and replay

### Requirement: Subscription command outcome events

The subscription command handler and workflows SHALL emit outcome events on the control-plane bus following the established `{Operation}Succeeded|Failed` convention: `SubscribeSucceeded|SubscribeFailed`, `UpdateSubscriptionSucceeded|UpdateSubscriptionFailed`, and `UnsubscribeSucceeded|UnsubscribeFailed`. Deterministic validation failures, including unknown or inactive domains, missing subscriber access, missing entitlement when required, path ownership mismatch, and rule-name constraint violations, SHALL emit the corresponding `Failed` outcome with the error reason and SHALL acknowledge rather than retry the SQS message.

#### Scenario: Authorization failure emits outcome event

- **WHEN** a create command fails because the caller lacks subscriber access or the consumer domain lacks a current covering entitlement
- **THEN** the handler SHALL emit `SubscribeFailed` with the authorization error
- **AND THEN** the message SHALL be acknowledged rather than retried

#### Scenario: Disable remains processable after entitlement revocation

- **WHEN** an update command disables a subscription whose last covering entitlement was revoked
- **THEN** the handler SHALL process the command rather than reject it for missing entitlement

#### Scenario: Enable without entitlement fails deterministically

- **WHEN** an update command enables a subscription whose consumer domain lacks a current covering entitlement
- **THEN** the handler SHALL emit `UpdateSubscriptionFailed`
- **AND THEN** the message SHALL be acknowledged rather than retried
