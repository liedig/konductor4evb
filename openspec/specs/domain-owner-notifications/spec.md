# Capability: Domain Owner Notifications

## Purpose

TBD

## Requirements

### Requirement: Approval request notification

When a domain registration enters the approval phase, the control plane SHALL send an email notification to the configured platform team address via Amazon SES. The email SHALL contain the domain name, owner account ID, domain owner email, and the task token needed to submit a decision. The notification SHALL be sent by a dedicated Lambda function invoked by the approval workflow via the `.waitForTaskToken` pattern.

#### Scenario: Approval notification sent during workflow wait

- **WHEN** the approval workflow enters the `NotifyAndWait` state
- **THEN** the workflow SHALL invoke the `SendApprovalNotification` Lambda with the task token, domain name, owner account ID, and owner email
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

Email notifications SHALL use Amazon SES with parameterised sender and recipient addresses. SES email identity verification SHALL be managed by a separate `notifications-stack.yaml` CloudFormation template, not by the Nexus core stack. The core template SHALL accept `ApprovalFromEmail` and `ApprovalToEmail` as optional parameters with empty-string defaults. When both parameters are non-empty, the core template SHALL deploy the notification Lambda functions, their log groups, and the rejection EventBridge rule. When either parameter is empty, the core template SHALL omit all notification resources. The notifications stack SHALL deploy in the same AWS region as the Nexus core stack because SES identities are regional.

#### Scenario: SES parameters defined in core template

- **WHEN** the Nexus core stack is deployed
- **THEN** the SAM template SHALL accept optional `ApprovalFromEmail` and `ApprovalToEmail` parameters with empty-string defaults
- **AND THEN** when both parameters are non-empty, they SHALL be passed as environment variables to the notification Lambda functions
- **AND THEN** when either parameter is empty, the notification Lambda functions and their EventBridge rules SHALL NOT be deployed

#### Scenario: Notifications stack provisions SES identities

- **WHEN** the `notifications-stack.yaml` template is deployed
- **THEN** it SHALL create an `AWS::SES::EmailIdentity` resource for the From address
- **AND THEN** it SHALL create an `AWS::SES::EmailIdentity` resource for the To address
- **AND THEN** it SHALL output the `FromEmail` and `ToEmail` values for use as input to the core stack

#### Scenario: SES sandbox for development

- **WHEN** the notifications stack is deployed in a development environment using SES sandbox mode
- **THEN** both the sender and recipient email addresses SHALL be verified in SES before notifications can be sent
- **AND THEN** unverified addresses SHALL result in SES sending failures, not silent drops

#### Scenario: Core stack deploys without SES

- **WHEN** the Nexus core stack is deployed with `ApprovalFromEmail` and `ApprovalToEmail` left as empty strings
- **THEN** the core stack SHALL deploy successfully without any notification Lambda functions or EventBridge rules
- **AND THEN** the approval workflow SHALL still execute and wait on a task token for a manual decision
- **AND THEN** operators SHALL be able to approve or reject registrations via the `POST /domains/registrations/decide` API endpoint

#### Scenario: Notifications stack deployed in a different region than core

- **WHEN** the notifications stack is deployed in a different AWS region than the Nexus core stack
- **THEN** SES identities created by the notifications stack SHALL NOT be usable by the core stack's notification Lambda functions
- **AND THEN** the template description and documentation SHALL call out the same-region constraint
