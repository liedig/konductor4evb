# Capability: Async Command Processing

## Purpose
TBD

## Requirements

### Requirement: Command queue topology

The control plane SHALL route all write API operations through SQS queues to provide durable, at-least-once command delivery. Two queues SHALL be provisioned, each with a dedicated dead-letter queue (DLQ): one for domain commands (POST/PUT on `/domains` and `/domains/{domainId}`) and one for registration commands (POST on `/domains/registrations/{requestId}/decision`).

#### Scenario: Domain commands routed to domains queue

- **WHEN** an authenticated caller sends a POST or PUT request to a domain write endpoint
- **THEN** API Gateway SHALL deliver the request body as an SQS message to the domains command queue
- **AND THEN** the SQS message SHALL include an `HttpMethod` message attribute carrying the original HTTP method
- **AND THEN** path parameters (e.g., `domainId`) SHALL be injected into the message body by the API Gateway VTL mapping template
- **AND THEN** API Gateway SHALL return `202 Accepted` with `{"message":"OK"}` immediately, without waiting for the command to be processed

#### Scenario: Registration commands routed to registrations queue

- **WHEN** an authenticated caller sends a POST request to the registration decision endpoint
- **THEN** API Gateway SHALL deliver the request body as an SQS message to the registrations command queue
- **AND THEN** the SQS message SHALL include an `HttpMethod` message attribute
- **AND THEN** the `requestId` path parameter SHALL be injected into the message body by the VTL mapping template
- **AND THEN** API Gateway SHALL return `202 Accepted` with `{"message":"OK"}` immediately

#### Scenario: Failed messages captured in DLQ

- **WHEN** a command handler Lambda fails to process a message after exhausting all SQS retry attempts (maxReceiveCount)
- **THEN** the message SHALL be moved to the queue's dedicated DLQ
- **AND THEN** the message SHALL be retained in the DLQ for the configured retention period (minimum 14 days)
- **AND THEN** an operator SHALL be able to inspect and replay messages from the DLQ

### Requirement: API Gateway structural validation

The control plane SHALL validate the structure of all write request bodies at the API Gateway layer before enqueueing them. Structural validation SHALL enforce required fields, field types, and format constraints defined in the OpenAPI schema. Business rule validation (uniqueness, state transitions) SHALL remain in the command handler Lambda.

#### Scenario: Structurally invalid request rejected before enqueueing

- **WHEN** a caller sends a write request with a missing required field or invalid field type
- **THEN** API Gateway SHALL reject the request with `400 Bad Request` synchronously
- **AND THEN** the malformed message SHALL NOT be placed on the SQS queue
- **AND THEN** the response SHALL include a descriptive validation error

#### Scenario: Structurally valid request accepted and enqueued

- **WHEN** a caller sends a write request that passes structural validation
- **THEN** API Gateway SHALL enqueue the message and return `202 Accepted`
- **AND THEN** business rule validation SHALL occur asynchronously in the command handler Lambda

### Requirement: Command handler dispatch

Each SQS queue SHALL be consumed by a dedicated command handler Lambda that uses the `HttpMethod` message attribute to dispatch to the appropriate operation handler. The command handler SHALL process messages and publish outcome events to the control plane EventBridge bus.

#### Scenario: Domain command handler dispatches by HTTP method

- **WHEN** the domain command handler receives an SQS message with `HttpMethod: POST`
- **THEN** it SHALL execute the register-domain logic
- **AND WHEN** it receives a message with `HttpMethod: PUT`
- **THEN** it SHALL execute the update-domain logic

#### Scenario: Registration command handler dispatches by HTTP method

- **WHEN** the registration command handler receives an SQS message with `HttpMethod: POST`
- **THEN** it SHALL execute the decide-registration logic

#### Scenario: Unknown HTTP method causes message failure

- **WHEN** a command handler receives a message with an unrecognised `HttpMethod` attribute
- **THEN** it SHALL return an error causing SQS to retry and eventually route to the DLQ
- **AND THEN** the handler SHALL log the unrecognised method for operator investigation

### Requirement: Command outcome events

Each command handler SHALL emit a specific outcome event to the control plane EventBridge bus upon successful or failed processing of a command. Outcome events SHALL follow the naming convention `{Operation}Succeeded` or `{Operation}Failed`.

#### Scenario: Successful command emits succeeded event

- **WHEN** a command handler successfully processes a command (e.g., domain registered, domain updated, registration decided)
- **THEN** it SHALL emit the corresponding `*Succeeded` event to the control plane EventBridge bus
- **AND THEN** the event SHALL include sufficient payload for downstream consumers to act without querying the API

#### Scenario: Failed command emits failed event

- **WHEN** a command handler encounters a business rule violation or processing error for a command (e.g., domain name already exists, invalid state transition)
- **THEN** it SHALL emit the corresponding `*Failed` event to the control plane EventBridge bus
- **AND THEN** the event SHALL include the error reason so downstream consumers can surface it to the caller

#### Scenario: Outcome events for each write operation

- **WHEN** the register-domain command completes
- **THEN** either `RegisterDomainSucceeded` or `RegisterDomainFailed` SHALL be emitted
- **AND WHEN** the update-domain command completes
- **THEN** either `UpdateDomainSucceeded` or `UpdateDomainFailed` SHALL be emitted
- **AND WHEN** the decide-registration command completes
- **THEN** either `DecideRegistrationSucceeded` or `DecideRegistrationFailed` SHALL be emitted
