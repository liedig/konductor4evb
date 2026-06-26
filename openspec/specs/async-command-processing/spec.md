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
