## MODIFIED Requirements

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
