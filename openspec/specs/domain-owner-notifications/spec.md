# Capability: Domain Owner Notifications

## Purpose
TBD

## Requirements

### Requirement: Approval request notification

When a domain registration enters the approval phase, the control plane SHALL send an email notification to the configured platform team address via Amazon SES. The email SHALL contain the domain name, owner account ID, domain owner email, and the task token needed to submit a decision. The notification SHALL be sent by a dedicated Lambda function invoked by the approval workflow via the `.waitForTaskToken` pattern.

#### Scenario: Approval notification sent during workflow wait

- **WHEN** the approval workflow enters the `NotifyAndWait` state
- **THEN** the workflow SHALL invoke the `SendApprovalNotification` Lambda with the task token, domain ID, domain name, owner account ID, and owner email
- **AND THEN** the Lambda SHALL persist the task token to the domain record in DynamoDB
- **AND THEN** the Lambda SHALL construct and send an email via SES to the configured platform team address
- **AND THEN** the email SHALL include the domain name, owner account ID, owner email, and the task token
- **AND THEN** the workflow SHALL block until a callback is received via `SendTaskSuccess` or `SendTaskFailure`

#### Scenario: Approval notification Lambda failure

- **WHEN** the `SendApprovalNotification` Lambda fails (SES error, DynamoDB error)
- **THEN** the workflow step SHALL fail and the workflow SHALL handle the error according to its error handling configuration
- **AND THEN** the task token SHALL NOT be persisted if the Lambda did not complete successfully

### Requirement: Rejection notification

When a domain registration is rejected, the control plane SHALL send an email notification to the domain owner informing them of the rejection and providing the justification. The notification SHALL be triggered by an EventBridge rule matching the `DomainRegistrationRejected` event and handled by a dedicated Lambda function.

#### Scenario: Rejection email sent to domain owner

- **WHEN** the approval workflow emits a `DomainRegistrationRejected` event to the control plane event bus
- **THEN** an EventBridge rule SHALL match the event and invoke the `SendRejectionNotification` Lambda
- **AND THEN** the Lambda SHALL send an email via SES to the `ownerEmail` from the event detail
- **AND THEN** the email SHALL include the domain name, the rejecting user (`rejectedBy`), and the justification

#### Scenario: Rejection notification Lambda failure

- **WHEN** the `SendRejectionNotification` Lambda fails to send the email
- **THEN** the Lambda SHALL log the error
- **AND THEN** the domain status SHALL NOT be affected (the rejection has already been recorded by the workflow)

### Requirement: SES configuration

Email notifications SHALL use Amazon SES with parameterised sender and recipient addresses. The SAM template SHALL define parameters for the approval sender address and the platform team recipient address. In development, SES sandbox mode SHALL be used with verified email addresses only.

#### Scenario: SES parameters defined in SAM template

- **WHEN** the control plane is deployed
- **THEN** the SAM template SHALL accept `ApprovalFromEmail` and `ApprovalToEmail` parameters
- **AND THEN** these parameters SHALL be passed as environment variables to the notification Lambda functions

#### Scenario: SES sandbox for development

- **WHEN** the control plane is deployed in a development environment using SES sandbox mode
- **THEN** both the sender and recipient email addresses SHALL be verified in SES before notifications can be sent
- **AND THEN** unverified addresses SHALL result in SES sending failures, not silent drops
