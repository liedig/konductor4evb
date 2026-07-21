# Capability: Domain Owner Notifications (delta)

## ADDED Requirements

### Requirement: Registration failure notification

When a domain registration command fails deterministic validation (duplicate name, naming rule violation), the control plane SHALL send an email notification to the submitted `ownerEmail` informing them that the registration was not accepted and why. The notification SHALL be triggered by an EventBridge rule matching the `RegisterDomainFailed` event and handled by a dedicated Lambda function, deployed under the same conditional-notification configuration as the other notification functions.

#### Scenario: Failure email sent to the submitted owner email

- **WHEN** the domain command handler emits a `RegisterDomainFailed` event to the control plane event bus
- **THEN** an EventBridge rule SHALL match the event and invoke the registration-failed notification Lambda
- **AND THEN** the Lambda SHALL send an email via SES to the `ownerEmail` from the event detail
- **AND THEN** the email SHALL include the domain name and the failure reason

#### Scenario: Notifications disabled

- **WHEN** the core stack is deployed without SES notification parameters
- **THEN** the registration-failed notification Lambda and its EventBridge rule SHALL NOT be deployed
- **AND THEN** `RegisterDomainFailed` events SHALL remain observable on the control plane bus

### Requirement: Permanent email failures are not retried

Notification Lambdas SHALL classify SES send failures as permanent or transient. Permanent failures (SES `MessageRejected` such as an unverified identity in sandbox mode, and `BadRequestException` such as a malformed address) SHALL be logged at ERROR with the SES error detail and acknowledged without retry, because retrying a deterministic rejection cannot succeed. Transient failures (throttling, service errors) SHALL propagate so the asynchronous invocation retry and failure DLQ behavior applies.

#### Scenario: Unverified recipient in SES sandbox

- **WHEN** a notification Lambda's SES send fails with `MessageRejected` because the recipient identity is not verified
- **THEN** the Lambda SHALL log the failure at ERROR including the SES error message
- **AND THEN** the invocation SHALL complete successfully (no retry, no failure-DLQ delivery)

#### Scenario: Transient SES error

- **WHEN** a notification Lambda's SES send fails with a throttling or service error
- **THEN** the invocation SHALL fail so that the asynchronous retry policy and Lambda failure DLQ behavior apply unchanged
