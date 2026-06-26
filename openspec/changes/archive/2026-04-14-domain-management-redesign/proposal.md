## Why

The current domain management architecture has structural issues that complicate the registration flow and create ambiguous responsibilities. The decide-registration path routes through an unnecessary SQS queue with a command handler that duplicates event emission already handled by the approval workflow. The two-table data model (domains + registration requests) splits what is logically a single entity across two tables with redundant status tracking. Status names are ambiguous (`FAILED`, `CREATING`, `DEACTIVATED`) and don't clearly communicate what happened or whose turn it is to act. The approval workflow lacks domain owner notification, and the decide endpoint exposes an async fire-and-forget pattern for what is fundamentally a synchronous human action. These issues need to be resolved before building further capabilities on top of domain management.

## What Changes

### Registration flow

- **BREAKING**: `POST /domains` response changes from `{"message":"OK"}` to include `requestId` (SQS message ID) and a descriptive message
- **BREAKING**: Add required `ownerEmail` field to the register domain command for domain owner notifications
- Terminology: "requester" is replaced by "domain owner" throughout

### Approval and decide flow

- **BREAKING**: Remove `POST /domains/registrations/{requestId}/decision` endpoint
- **BREAKING**: Add new `POST /domains/registrations/decide` sync endpoint (API Gateway → Lambda, no SQS) accepting `taskToken`, `decision`, `decidedBy`, and `justification` in the request body
- Remove `DecideRegistrationSucceeded` and `DecideRegistrationFailed` events — the approval workflow is the sole owner of approval/rejection event emission
- Remove the registrations SQS command queue and its DLQ
- Remove the registration command handler Lambda
- Add `decidedBy` and `justification` fields to the approval workflow state, DynamoDB updates, and emitted events

### Notification system

- Add `SendApprovalNotification` Lambda invoked by the approval workflow via `.waitForTaskToken` pattern — persists the task token to DynamoDB and sends an approval request email to the platform team via Amazon SES
- Add rejection notification handler Lambda triggered by `DomainRegistrationRejected` EventBridge rule — sends rejection email with justification to the domain owner via SES

### Data model

- **BREAKING**: Merge the registration requests table into the domains table — single table, single record per domain, status-driven lifecycle
- Remove the `RegistrationRequest` Rust model from the common library
- `taskToken` is ephemeral, stored on the domain record during `PENDING_APPROVAL`, never exposed in API responses to domain owners, accessible to platform team callers

### Domain status model

- **BREAKING**: Replace status enum with: `PENDING_APPROVAL`, `APPROVED`, `REJECTED`, `PROVISIONING`, `ACTIVE`, `PROVISIONING_FAILED`, `INACTIVE`
- Remove: `CREATING` (replaced by `PROVISIONING`), `SUSPENDED` (folded into future `INACTIVE` workflow), `DEACTIVATED` (replaced by `INACTIVE`), `FAILED` (replaced by `PROVISIONING_FAILED`)
- `PROVISIONING` status set explicitly by the provisioning workflow as its first step

### Update command scope

- **BREAKING**: Remove `desiredStatus` from the update domain request — updates are metadata-only (description, tags)
- Domain deactivation/inactivation is a separate future workflow with its own API (out of scope for this change)

### Step Functions

- **BREAKING**: All state machine definitions converted from JSON to YAML (`.asl.json` → `.asl.yaml`)
- **BREAKING**: All state machines converted from JSONPath to JSONata query language

## Capabilities

### New Capabilities

- `domain-owner-notifications`: SES-based email notifications to domain owners and platform approvers at key lifecycle transitions (approval request, rejection with justification, provisioning completion)

### Modified Capabilities

- `domain-management`: Revised status model, single-table data model, `ownerEmail` field, removal of `desiredStatus` from updates, revised API surface (new decide endpoint, removed old decision endpoint, updated register response)
- `async-command-processing`: Removal of registrations command queue and handler — only the domains command queue remains. Decide flow moves to synchronous API Gateway → Lambda
- `domain-provisioning-workflow`: JSONata query language, YAML format, explicit `PROVISIONING` status transition at workflow start, `PROVISIONING_FAILED` on error
- `system-architecture`: Step Functions convention change to YAML + JSONata. SES added as a service dependency

## Impact

### Code changes

- **Remove**: registration command handler Lambda, `RegistrationRequest` model, registrations SQS queue/DLQ definitions
- **Add**: decide Lambda (sync, behind API Gateway), `SendApprovalNotification` Lambda, rejection notification handler Lambda
- **Modify**: domain command handler (register adds `ownerEmail`, update removes `desiredStatus`), all query Lambdas (single table, strip `taskToken`), common library (`DomainStatus` enum, model changes)

### API changes

- OpenAPI spec: remove `/domains/registrations/{requestId}/decision`, add `/domains/registrations/decide`, update `RegisterDomainRequest` schema (add `ownerEmail`), update `UpdateDomainRequest` schema (remove `desiredStatus`), update `CommandAccepted` response schema
- `GET /domains/registrations` may be replaced by `GET /domains?status=PENDING_APPROVAL`

### Infrastructure

- SAM template: remove registrations queue/DLQ, add decide Lambda + API Gateway integration, add notification Lambdas, add SES configuration (parameterised sender/recipient), remove registration requests DynamoDB table, update domains table attributes
- State machines: rewrite both approval and provisioning workflows (YAML + JSONata)

### Dependencies

- New: Amazon SES (sandbox for development, verified addresses only)
- Removed: registrations SQS queue and associated IAM roles
