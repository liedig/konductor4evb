## 1. Data Model and Common Library

- [x] 1.1 Update `DomainStatus` enum in `lib/control-plane-common/src/models.rs`: replace variants with `PendingApproval`, `Approved`, `Rejected`, `Provisioning`, `Active`, `ProvisioningFailed`, `Inactive` and update the `Display` impl
- [x] 1.2 Add `ownerEmail` field to `RegisterDomainRequest` and `Domain` models
- [x] 1.3 Add approval-phase fields to `Domain` model: `task_token`, `decided_by`, `justification`, `approved_at`, `rejected_at`
- [x] 1.4 Remove `RegistrationRequest` model and `DecisionRequest` model from `models.rs`
- [x] 1.5 Remove `desiredStatus` handling from update-related models (if any exist in common lib)
- [x] 1.6 Add `DecideRequest` model with fields: `task_token`, `decision`, `decided_by`, `justification`

## 2. SAM Template — Remove Old Resources

- [x] 2.1 Remove `RegistrationRequestsTable` DynamoDB table definition
- [x] 2.2 Remove `RegistrationsCommandQueue` and `RegistrationCommandDlq` SQS queue definitions
- [x] 2.3 Remove `RegistrationCommandHandlerFunction` Lambda definition and its IAM role/policies
- [x] 2.4 Remove the EventBridge rule for `RegistrationsCommandQueue` event source mapping (if any)
- [x] 2.5 Update `DomainsTable` to add approval-phase attributes (`taskToken`, `decidedBy`, `justification`, `approvedAt`, `rejectedAt`, `ownerEmail`, `errorDetail`) — note: DynamoDB is schemaless, but ensure GSIs and any attribute definitions are correct

## 3. SAM Template — Add New Resources

- [x] 3.1 Add `DecideFunction` Lambda definition with `aws_proxy` API Gateway integration for `POST /domains/registrations/decide`
- [x] 3.2 Add `SendApprovalNotificationFunction` Lambda definition with IAM permissions for DynamoDB (update domain record) and SES (send email)
- [x] 3.3 Add `SendRejectionNotificationFunction` Lambda definition with IAM permissions for SES (send email)
- [x] 3.4 Add EventBridge rule on control plane bus matching `DomainRegistrationRejected` → triggers `SendRejectionNotificationFunction`
- [x] 3.5 Add SES parameters (`ApprovalFromEmail`, `ApprovalToEmail`) to SAM template parameters section
- [x] 3.6 Pass SES parameters as environment variables to notification Lambda functions
- [x] 3.7 Add `ses:SendEmail` permission to notification Lambda execution roles

## 4. OpenAPI Spec

- [x] 4.1 Add `ownerEmail` (required, email format) to `RegisterDomainRequest` schema
- [x] 4.2 Remove `desiredStatus` from `UpdateDomainRequest` schema
- [x] 4.3 Remove `/domains/registrations/{requestId}/decision` path and its `DecisionRequest` schema
- [x] 4.4 Add `POST /domains/registrations/decide` path with `aws_proxy` Lambda integration and `DecideRequest` schema (taskToken, decision, decidedBy, justification)
- [x] 4.5 Update `CommandAccepted` response schema to include `requestId` field alongside `message`
- [x] 4.6 Update VTL response mapping template for SQS-backed endpoints to return `{"requestId":"<SQS-MessageId>","message":"..."}` from the SQS response
- [x] 4.7 Remove `/domains/registrations` path (or update docs to indicate callers should use `GET /domains?status=PENDING_APPROVAL`)
- [x] 4.8 Remove `RegistrationRequestResponse` and `ListRegistrationRequestsResponse` schemas

## 5. Domain Command Handler — Register

- [x] 5.1 Update `register.rs` to accept and persist `ownerEmail` field on the domain record
- [x] 5.2 Update `register.rs` to write only to the domains table (remove any registration requests table writes)
- [x] 5.3 Update the approval workflow input to include `ownerEmail`
- [x] 5.4 Update `RegisterDomainSucceeded` event payload to include `ownerEmail`

## 6. Domain Command Handler — Update

- [x] 6.1 Remove `desiredStatus` handling from `update.rs` — no status transitions, metadata only (description, tags)
- [x] 6.2 Remove `DomainDeactivated` event emission from `update.rs`
- [x] 6.3 Remove EventBridge policy removal logic (was part of deactivation flow) from `update.rs`

## 7. Decide Lambda (New)

- [x] 7.1 Create new Cargo binary target at `domain-management/functions/decide-registration/`
- [x] 7.2 Add to workspace members in root `Cargo.toml`
- [x] 7.3 Implement the decide handler: parse API Gateway proxy event body, validate fields (taskToken, decision, decidedBy, justification required on REJECT)
- [x] 7.4 Implement `ACCEPT` path: call `sfn.send_task_success(taskToken, output)` with `{approved: true, decidedBy, justification}`
- [x] 7.5 Implement `REJECT` path: call `sfn.send_task_failure(taskToken, error, cause)` with `{approved: false, decidedBy, justification}`
- [x] 7.6 Return `200 OK` with `{"message":"Decision recorded"}` on success, appropriate error responses on failure

## 8. SendApprovalNotification Lambda (New)

- [x] 8.1 Create new Cargo binary target at `domain-management/functions/send-approval-notification/`
- [x] 8.2 Add to workspace members in root `Cargo.toml`
- [x] 8.3 Implement: receive payload from Step Functions (taskToken, domainId, domainName, ownerAccountId, ownerEmail)
- [x] 8.4 Persist taskToken to the domain record in DynamoDB via UpdateItem
- [x] 8.5 Construct HTML approval notification email with domain details and task token
- [x] 8.6 Send email via SES to configured platform team address (`ApprovalToEmail` env var)

## 9. SendRejectionNotification Lambda (New)

- [x] 9.1 Create new Cargo binary target at `domain-management/functions/send-rejection-notification/`
- [x] 9.2 Add to workspace members in root `Cargo.toml`
- [x] 9.3 Implement: receive EventBridge event payload (domainName, ownerEmail, rejectedBy, justification)
- [x] 9.4 Construct HTML rejection email with domain name, rejector, and justification
- [x] 9.5 Send email via SES to `ownerEmail` from the event detail

## 10. Query Lambdas

- [x] 10.1 Update `list-domains` to read from the single domains table only (remove any registration requests table queries)
- [x] 10.2 Update `list-domains` to strip `taskToken` from response items
- [x] 10.3 Update `describe-domain` to strip `taskToken` from response (domain owner callers)
- [x] 10.4 Update `describe-domain` to include approval metadata (`decidedBy`, `justification`, timestamps) when available
- [x] 10.5 Remove `list-pending-activations` Lambda — delete the function directory and remove from workspace members and SAM template

## 11. Approval Workflow (Rewrite)

- [x] 11.1 Create `domain-management/state-machines/approval.asl.yaml` with `QueryLanguage: JSONata`
- [x] 11.2 Implement `NotifyAndWait` state using `.waitForTaskToken` invoking `SendApprovalNotificationFunction` with `$states.context.Task.Token`
- [x] 11.3 Implement `CheckDecision` choice state using JSONata condition `{% $states.input.approved = true %}`
- [x] 11.4 Implement `UpdateStatusApproved` state: DynamoDB UpdateItem on domains table setting status to `APPROVED`, `decidedBy`, `justification`, `approvedAt`
- [x] 11.5 Implement `UpdateStatusRejected` state: DynamoDB UpdateItem on domains table setting status to `REJECTED`, `decidedBy`, `justification`, `rejectedAt`
- [x] 11.6 Implement `EmitApprovedEvent` state: PutEvents with `DomainRegistrationApproved` including `approvedBy`, `justification`, `ownerEmail`
- [x] 11.7 Implement `EmitRejectedEvent` state: PutEvents with `DomainRegistrationRejected` including `rejectedBy`, `justification`, `ownerEmail`
- [x] 11.8 Delete old `domain-management/state-machines/approval.asl.json`

## 12. Provisioning Workflow (Rewrite)

- [x] 12.1 Create `domain-management/state-machines/provisioning.asl.yaml` with `QueryLanguage: JSONata`
- [x] 12.2 Add `SetProvisioningStatus` as the first step: DynamoDB UpdateItem setting domain status to `PROVISIONING`
- [x] 12.3 Convert all existing provisioning steps from JSONPath to JSONata (CreatePlatformEventBus, SetPutEventPolicy, CreateSchemaRegistry, CreateSubscriptionDlq, AssumeAppAccountRole, CreateAppEventBus, CreateIntegrationDlq, CreateDashboard, alarms)
- [x] 12.4 Update `UpdateDomainRecord` success step to set status to `ACTIVE`
- [x] 12.5 Update `UpdateDomainFailed` step to set status to `PROVISIONING_FAILED`
- [x] 12.6 Refactor the CloudWatch dashboard JSON body from `States.Format` to a readable JSONata expression
- [x] 12.7 Delete old `domain-management/state-machines/provisioning.asl.json`

## 13. Cleanup and Wiring

- [x] 13.1 Remove `registration-command-handler` directory and its Cargo.toml from workspace members
- [x] 13.2 Update SAM template EventBridge rules: ensure `DomainRegistrationApproved` rule targets provisioning workflow, `DomainRegistrationRejected` rule targets rejection notification Lambda
- [x] 13.3 Verify all Lambda environment variables reference the correct single domains table name
- [x] 13.4 Verify IAM roles: decide Lambda needs `states:SendTaskSuccess` and `states:SendTaskFailure` permissions

## 14. Documentation

- [x] 14.1 Update decision log (`docs/development/decisions.md`) with key decisions from this change: single domain table, sync decide endpoint, workflow-owned events, YAML + JSONata, SES notifications, metadata-only updates
