## MODIFIED Requirements

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
