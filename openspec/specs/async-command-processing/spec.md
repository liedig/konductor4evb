# Capability: Async Command Processing

## Purpose

TBD

## Requirements

### Requirement: Command queue topology

The control plane SHALL route domain write API operations (POST and PUT on `/domains` and `/domains/{domainName}`) through an SQS queue to provide durable, at-least-once command delivery. One command queue SHALL be provisioned with a dedicated dead-letter queue (DLQ) for domain commands. There SHALL NOT be a separate registrations command queue. The decide-registration operation SHALL NOT use SQS — it is handled by a synchronous Lambda endpoint.

#### Scenario: Domain commands routed to domains queue

- **WHEN** an authenticated caller sends a POST or PUT request to a domain write endpoint (`/domains` or `/domains/{domainName}`)
- **THEN** API Gateway SHALL deliver the request body as an SQS message to the domains command queue
- **AND THEN** the SQS message SHALL include an `HttpMethod` message attribute carrying the original HTTP method
- **AND THEN** path parameters (e.g., `domainName`) SHALL be injected into the message body by the API Gateway VTL mapping template
- **AND THEN** API Gateway SHALL return `202 Accepted` with `{"requestId":"<SQS-MessageId>","message":"..."}` immediately, without waiting for the command to be processed

#### Scenario: Decide-registration handled synchronously

- **WHEN** an authenticated caller sends a POST request to `/domains/registrations/decide`
- **THEN** API Gateway SHALL route the request directly to the decide Lambda function via `aws_proxy` integration
- **AND THEN** the request SHALL NOT be placed on any SQS queue

#### Scenario: Failed messages captured in DLQ

- **WHEN** the domain command handler Lambda fails to process a message after exhausting all SQS retry attempts (maxReceiveCount)
- **THEN** the message SHALL be moved to the queue's dedicated DLQ
- **AND THEN** the message SHALL be retained in the DLQ for the configured retention period (minimum 14 days)
- **AND THEN** an operator SHALL be able to inspect and replay messages from the DLQ

### Requirement: API Gateway structural validation

The control plane SHALL validate the structure of all write request bodies at the API Gateway layer before processing. For SQS-backed endpoints, structural validation SHALL occur before enqueueing. For the synchronous decide endpoint, structural validation SHALL occur before Lambda invocation. Structural validation SHALL enforce required fields, field types, and format constraints defined in the OpenAPI schema. Business rule validation (uniqueness, state transitions) SHALL remain in the command handler or decide Lambda.

#### Scenario: Structurally invalid request rejected before enqueueing

- **WHEN** a caller sends a write request to an SQS-backed endpoint with a missing required field or invalid field type
- **THEN** API Gateway SHALL reject the request with `400 Bad Request` synchronously
- **AND THEN** the malformed message SHALL NOT be placed on the SQS queue
- **AND THEN** the response SHALL include a descriptive validation error

#### Scenario: Structurally valid request accepted and enqueued

- **WHEN** a caller sends a write request to an SQS-backed endpoint that passes structural validation
- **THEN** API Gateway SHALL enqueue the message and return `202 Accepted`
- **AND THEN** business rule validation SHALL occur asynchronously in the command handler Lambda

#### Scenario: Structurally invalid decide request rejected

- **WHEN** a caller sends a POST to `/domains/registrations/decide` with missing required fields
- **THEN** API Gateway SHALL validate the request body against the OpenAPI schema
- **AND THEN** reject invalid requests with `400 Bad Request` before invoking the decide Lambda

### Requirement: Command handler dispatch

The domains SQS queue SHALL be consumed by a dedicated command handler Lambda that uses the `HttpMethod` message attribute to dispatch to the appropriate operation handler. The command handler SHALL process messages and publish outcome events to the control plane EventBridge bus. There SHALL NOT be a separate registration command handler.

#### Scenario: Domain command handler dispatches by HTTP method

- **WHEN** the domain command handler receives an SQS message with `HttpMethod: POST`
- **THEN** it SHALL execute the register-domain logic
- **AND WHEN** it receives a message with `HttpMethod: PUT`
- **THEN** it SHALL execute the update-domain logic

#### Scenario: Unknown HTTP method causes message failure

- **WHEN** a command handler receives a message with an unrecognised `HttpMethod` attribute
- **THEN** it SHALL return an error causing SQS to retry and eventually route to the DLQ
- **AND THEN** the handler SHALL log the unrecognised method for operator investigation

### Requirement: Command outcome events

The domain command handler SHALL emit a specific outcome event to the control plane EventBridge bus upon successful or failed processing of a command. Outcome events SHALL follow the naming convention `{Operation}Succeeded` or `{Operation}Failed`. Only register-domain and update-domain operations emit command outcome events. The decide-registration operation does NOT emit command outcome events — the approval workflow handles event emission for approval and rejection.

#### Scenario: Successful command emits succeeded event

- **WHEN** the domain command handler successfully processes a command (e.g., domain registered, domain updated)
- **THEN** it SHALL emit the corresponding `*Succeeded` event to the control plane EventBridge bus
- **AND THEN** the event SHALL include sufficient payload for downstream consumers to act without querying the API

#### Scenario: Failed command emits failed event

- **WHEN** the domain command handler encounters a business rule violation or processing error for a command
- **THEN** it SHALL emit the corresponding `*Failed` event to the control plane EventBridge bus
- **AND THEN** the event SHALL include the error reason so downstream consumers can surface it to the caller

#### Scenario: Outcome events for domain operations only

- **WHEN** the register-domain command completes
- **THEN** either `RegisterDomainSucceeded` or `RegisterDomainFailed` SHALL be emitted
- **AND WHEN** the update-domain command completes
- **THEN** either `UpdateDomainSucceeded` or `UpdateDomainFailed` SHALL be emitted

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

### Requirement: Trusted principal propagation for queued commands

Every authenticated API surface that uses a direct API Gateway-to-SQS service integration SHALL inject a compact trusted principal context into the queued command. The mapping template SHALL construct this context only from API Gateway authentication or authorizer context and SHALL NOT trust identity, group, role, or account assertions from the request body. For OAuth, the queue envelope SHALL preserve the Cognito subject, mapped global roles, and trusted team-group identifiers. For IAM, it SHALL preserve the AWS account as `principalId` and resource-binding identity and MAY preserve the IAM principal ARN only as audit provenance. The envelope SHALL NOT include resource entitlements.

#### Scenario: OAuth command is enqueued

- **WHEN** an OAuth-authenticated request passes structural validation on an SQS-backed write route
- **THEN** the API Gateway mapping template SHALL enqueue the command through the existing direct SQS service integration
- **AND THEN** the command SHALL contain trusted OAuth subject, global-role, and team-group context derived from the REQUEST authorizer
- **AND THEN** caller-provided identity or role fields SHALL NOT override that context

#### Scenario: IAM command is enqueued

- **WHEN** an AWS_IAM-authenticated request passes structural validation on an SQS-backed write route
- **THEN** the API Gateway mapping template SHALL enqueue the command through the direct SQS service integration
- **AND THEN** the command SHALL use the authenticated AWS account as its trusted IAM `principalId` and resource-binding identity
- **AND THEN** an IAM principal ARN MAY be included only as audit provenance

#### Scenario: Consumer receives a normalized principal

- **WHEN** a queue consumer processes a command from either authenticated API surface
- **THEN** it SHALL receive the common trusted-principal envelope
- **AND THEN** OAuth team-group identifiers SHALL remain available to later resource-binding evaluation without being interpreted as entitlements by this capability
- **AND THEN** IAM authorization SHALL remain account-granular even when principal ARN audit provenance is present

### Requirement: Durable acceptance is not resource authorization

A `202 Accepted` response from an SQS-backed route SHALL mean only that the structurally valid command and trusted principal context were durably accepted by the queue. API Gateway and the OAuth authorizer SHALL NOT perform request-body or resource-level authorization for the queued command. The queue consumer SHALL perform resource authorization before applying a state change, using the common DynamoDB-backed authorization contract validated by the authorization-engine spike.

#### Scenario: Structurally valid but unauthorized command is accepted

- **WHEN** a structurally valid command is enqueued successfully but the principal lacks permission for the target resource
- **THEN** API Gateway SHALL return `202 Accepted` for durable queue acceptance
- **AND THEN** the queue consumer SHALL reject the command during resource authorization
- **AND THEN** no protected resource mutation SHALL occur

#### Scenario: Authorizer does not inspect command body

- **WHEN** the OAuth authorizer evaluates an SQS-backed request
- **THEN** it SHALL decide token and coarse route access without using body fields to authorize the target resource
- **AND THEN** the direct API Gateway-to-SQS integration SHALL remain in place
