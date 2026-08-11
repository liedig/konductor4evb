# Capability: Domain Owner Notifications

## Purpose

How the platform communicates registration lifecycle events by email: SES-based notifications for approval requests, rejections, and provisioning outcomes, sent by dedicated Lambda functions inside the approval workflow, with authenticated decision-making and no bearer-token links.

## Requirements

### Requirement: Approval request notification

When a domain registration enters the approval phase, the control plane SHALL send an email notification to the configured platform team address via Amazon SES. The email SHALL contain the domain name, owner account ID, and domain owner email and SHALL direct the approver to the authenticated OAuth approval experience. The email SHALL NOT expose the Step Functions task token as a bearer credential or provide an unauthenticated decision link. The notification SHALL be sent by a dedicated Lambda function invoked by the approval workflow via the `.waitForTaskToken` pattern. The Lambda SHALL continue to persist the task token as workflow callback state on the domain record.

#### Scenario: Approval notification sent during workflow wait

- **WHEN** the approval workflow enters the `NotifyAndWait` state
- **THEN** the workflow SHALL invoke the `SendApprovalNotification` Lambda with the task token, domain name, owner account ID, and owner email
- **AND THEN** the Lambda SHALL persist the task token to the domain record in DynamoDB
- **AND THEN** the Lambda SHALL construct and send an email via SES to the configured platform team address
- **AND THEN** the email SHALL include the domain name, owner account ID, and owner email
- **AND THEN** the email SHALL direct the approver to sign in through the Cognito OAuth experience before making a decision
- **AND THEN** the email SHALL NOT contain the task token or an unauthenticated approval URL
- **AND THEN** the workflow SHALL block until an authorized callback is received via `SendTaskSuccess` or `SendTaskFailure`

#### Scenario: Approval notification Lambda failure

- **WHEN** the `SendApprovalNotification` Lambda fails because of an SES or DynamoDB error
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

### Requirement: Provisioning succeeded notification

When domain provisioning completes successfully, the control plane SHALL send an email notification to the domain owners informing them that their domain is live. The notification SHALL be triggered by an EventBridge rule matching the `DomainProvisioningSucceeded` event and handled by a dedicated Lambda function (`send-provisioning-succeeded-notification`), deployed under the same conditional-notification configuration as the other notification functions. Recipients SHALL be resolved by loading the domain record from DynamoDB using the `domainName` from the event detail.

#### Scenario: Domain-live email sent to owners

- **WHEN** the provisioning workflow emits a `DomainProvisioningSucceeded` event to the nexus bus
- **THEN** an EventBridge rule SHALL match the event and invoke the provisioning-succeeded notification Lambda
- **AND THEN** the Lambda SHALL load the domain record from DynamoDB by `domainName` and send an email via SES to the owner recipients
- **AND THEN** the email SHALL state that the domain is provisioned and active and include the domain name

#### Scenario: Notifications disabled

- **WHEN** the core stack is deployed without notification parameters
- **THEN** the provisioning-succeeded notification Lambda and its EventBridge rule SHALL NOT be deployed
- **AND THEN** `DomainProvisioningSucceeded` events SHALL remain observable on the nexus bus

### Requirement: Registration delayed notification

When the approval request could not be delivered to the platform team, the control plane SHALL send an email notification to the domain owners informing them that their registration is delayed and that the platform team has been notified. The notification SHALL be triggered by an EventBridge rule matching the `DomainApprovalNotificationFailed` event and handled by a dedicated Lambda function (`send-registration-delayed-notification`), deployed under the same conditional-notification configuration. Recipients SHALL be resolved by loading the domain record from DynamoDB using the `domainName` from the event detail. The email SHALL NOT include technical failure detail. Owner delivery SHALL be independent of operator delivery: a failure to email the owners SHALL NOT affect the operator notification path.

#### Scenario: Registration-delayed email sent to owners

- **WHEN** the approval workflow emits a `DomainApprovalNotificationFailed` event to the nexus bus
- **THEN** an EventBridge rule SHALL match the event and invoke the registration-delayed notification Lambda
- **AND THEN** the Lambda SHALL load the domain record from DynamoDB by `domainName` and send an email via SES to the owner recipients
- **AND THEN** the email SHALL state that the registration is delayed and the platform team has been notified, without technical error detail

#### Scenario: Owner email fails for the same SES reason

- **WHEN** the registration-delayed email send fails permanently (e.g. the same SES misconfiguration that caused the original event)
- **THEN** the Lambda SHALL log the failure at ERROR and acknowledge without retry, per the permanent-failure classification
- **AND THEN** the operator SHALL still be notified via the platform notifications topic, which does not depend on SES

### Requirement: Owner notification recipients include secondary owners

Every owner-facing notification email (approval outcome, rejection outcome, provisioning failed, registration failed, provisioning succeeded, registration delayed) SHALL be addressed to the domain's `ownerEmail` and all `secondaryOwnerEmails` in a single SES send. Notification Lambdas that resolve recipients from the event detail SHALL read `secondaryOwnerEmails` from the detail; Lambdas that resolve recipients from the domain record SHALL read `secondaryOwnerEmails` from DynamoDB. A missing or empty `secondaryOwnerEmails` collection SHALL degrade to owner-only addressing, not an error. To support this, the `RegisterDomainFailed` event detail SHALL additively include `secondaryOwnerEmails` from the registration request, because no domain record exists for failed registrations.

#### Scenario: Secondary owners receive outcome notifications

- **WHEN** any owner-facing notification Lambda sends an email for a domain registered with secondary owner emails
- **THEN** the SES send SHALL list `ownerEmail` and every `secondaryOwnerEmails` entry in `to_addresses`

#### Scenario: No secondary owners registered

- **WHEN** a notification is sent for a domain whose `secondaryOwnerEmails` is absent or empty
- **THEN** the email SHALL be sent to `ownerEmail` only and the invocation SHALL succeed

#### Scenario: RegisterDomainFailed carries secondary owner emails

- **WHEN** the registration command handler emits `RegisterDomainFailed` for a request that included secondary owner emails
- **THEN** the event detail SHALL include `secondaryOwnerEmails`
- **AND THEN** the registration-failed notification Lambda SHALL address the email to the owner and secondary owners from the event detail
