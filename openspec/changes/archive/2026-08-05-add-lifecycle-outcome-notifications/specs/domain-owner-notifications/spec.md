# Delta: domain-owner-notifications

## ADDED Requirements

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
