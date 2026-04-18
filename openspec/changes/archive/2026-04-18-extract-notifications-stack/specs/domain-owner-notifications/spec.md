## MODIFIED Requirements

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
