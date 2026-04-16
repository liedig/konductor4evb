# Capability: Domain Management

## Requirements

### Requirement: Domain and request persistence

All domain details, lifecycle state, and approval metadata SHALL be stored in a single Amazon DynamoDB domains table. The control plane SHALL use DynamoDB as the system of record for domain metadata, resource identifiers, approval decisions, and the lifecycle status of each domain. There SHALL NOT be a separate registration requests table.

#### Scenario: Domain data stored in single table

- **WHEN** any domain is created, updated, approved, rejected, or provisioned
- **THEN** the control plane SHALL persist all domain details and status changes to the domains table
- **AND THEN** list and describe operations SHALL read from the domains table so that users see a consistent view of domains and their lifecycle state

#### Scenario: Approval-phase fields stored on domain record

- **WHEN** a domain registration enters the approval phase
- **THEN** the approval workflow SHALL write `taskToken`, `decidedBy`, `justification`, and decision timestamps directly to the domain record in the domains table
- **AND THEN** these fields SHALL coexist with the domain metadata fields on the same record

### Requirement: Domain management events

The domain management capability SHALL emit the following events. Each event SHALL have a well-defined type, a consistent payload, and a defined emitter (control plane command handler or workflow). Events SHALL be emitted on the control plane event bus. Command outcome events (`*Succeeded` / `*Failed`) are the primary mechanism by which callers learn the result of write operations. The approval workflow SHALL be the sole emitter of approval and rejection events.

#### Events emitted by the control plane command handler

| Event type | When emitted | Purpose | Minimum payload |
|------------|--------------|---------|-----------------|
| `RegisterDomainSucceeded` | After the register-domain command is successfully processed | Confirms domain registration was accepted | `domainId`, `domainName`, `ownerAccountId`, `ownerEmail`, `status`, `createdAt` |
| `RegisterDomainFailed` | After the register-domain command fails (e.g., duplicate name, validation error) | Surfaces processing failure to callers and audit | `domainName`, `ownerAccountId`, `reason`, `failedAt` |
| `UpdateDomainSucceeded` | After the update-domain command is successfully processed | Confirms domain metadata update was applied | `domainId`, `updatedAt`, `changedFields` |
| `UpdateDomainFailed` | After the update-domain command fails (e.g., domain not found) | Surfaces processing failure to callers and audit | `domainId`, `reason`, `failedAt` |

#### Events emitted by the approval workflow

| Event type | When emitted | Purpose | Minimum payload |
|------------|--------------|---------|-----------------|
| `DomainRegistrationApproved` | After the approval workflow processes an approval decision | Triggers the provisioning workflow | `domainId`, `domainName`, `ownerAccountId`, `ownerEmail`, `approvedBy`, `justification`, `approvedAt` |
| `DomainRegistrationRejected` | After the approval workflow processes a rejection decision | Triggers rejection notification; audit | `domainId`, `domainName`, `ownerAccountId`, `ownerEmail`, `rejectedBy`, `justification`, `rejectedAt` |

#### Events emitted by the provisioning workflow

| Event type | When emitted | Purpose | Minimum payload |
|------------|--------------|---------|-----------------|
| `DomainProvisioningSucceeded` | When the provisioning workflow completes successfully | Control plane updates domain to ACTIVE with resource ARNs | `domainId`, `workflowExecutionId`, `completedAt`, `resourceArns`, `namespaceParameter`, `isNewAccount` |
| `DomainProvisioningFailed` | When the provisioning workflow fails | Control plane updates domain to PROVISIONING_FAILED | `domainId`, `workflowExecutionId`, `failedAt`, `errorCode`, `errorMessage`, `compensationResults` |

#### Scenario: Approval workflow is sole emitter of approval/rejection events

- **WHEN** a registration decision is submitted via the decide endpoint
- **THEN** the decide Lambda SHALL call `SendTaskSuccess` or `SendTaskFailure` on the Step Functions workflow
- **AND THEN** the approval workflow SHALL emit `DomainRegistrationApproved` or `DomainRegistrationRejected`
- **AND THEN** no other component SHALL emit approval or rejection events

#### Scenario: Command outcome events emitted for write operations

- **WHEN** a write command (register, update) is processed by the command handler
- **THEN** the handler SHALL emit either a `*Succeeded` or `*Failed` outcome event to the control plane bus
- **AND THEN** callers and downstream systems SHALL subscribe to these events to learn the result of their commands

### Requirement: Register Domain

Domain management is an administrative function of the control plane. Domain owners SHALL be able to request registration of a new domain by submitting a command that is processed asynchronously. The API SHALL accept the command and return immediately with a request ID (SQS message ID) and a descriptive message. Each request SHALL go through an approval process; only after approval SHALL the provisioning workflow be triggered. All domain details SHALL be stored in the single domains DynamoDB table.

#### Scenario: Registration command accepted and enqueued

- **WHEN** an authorized domain owner calls the register-domain API with a valid domain name, owner account ID, owner email, and optional description and tags
- **THEN** API Gateway SHALL validate the request structure and enqueue the command to the domains SQS queue
- **AND THEN** the API SHALL return `202 Accepted` with `{"requestId":"<SQS-MessageId>","message":"Domain registration request accepted"}`

#### Scenario: Registration command processed and succeeded event emitted

- **WHEN** the domain command handler processes a register-domain message from the queue
- **THEN** the handler SHALL validate business rules (naming, uniqueness)
- **AND THEN** persist the domain in DynamoDB with status `PENDING_APPROVAL`
- **AND THEN** start the approval Step Functions workflow
- **AND THEN** emit `RegisterDomainSucceeded` to the control plane event bus

#### Scenario: Registration command fails business validation

- **WHEN** the domain command handler processes a register-domain message and a business rule is violated (e.g., duplicate domain name)
- **THEN** the handler SHALL emit `RegisterDomainFailed` to the control plane event bus with the reason
- **AND THEN** NOT persist any domain record or start any workflow

#### Scenario: Registration request rejected due to structural validation

- **WHEN** a caller invokes the register-domain API with missing required fields or invalid field formats
- **THEN** API Gateway SHALL reject the request synchronously with `400 Bad Request`
- **AND THEN** the malformed request SHALL NOT be enqueued

### Requirement: List Domains

Users SHALL be able to list the domains they have permission to access, with filters and pagination that scale to large organizations.

#### Scenario: List domains with default filters

- **WHEN** a user calls the list-domains API with no filters
- **THEN** the control plane SHALL return all domains the caller is authorized to see
- **AND THEN** include basic metadata such as name, description, owner, creation date, and status for each domain
- **AND THEN** return results in a paginated form that can scale to thousands of domains
- **AND THEN** the `taskToken` field SHALL NOT be included in the response

#### Scenario: List domains with filters

- **WHEN** a user calls the list-domains API with filters such as owner account, tags, or status
- **THEN** the control plane SHALL apply those filters server-side
- **AND THEN** return only matching domains along with pagination tokens where needed

#### Scenario: List domains filtered by PENDING_APPROVAL status

- **WHEN** a user calls the list-domains API with `status=PENDING_APPROVAL`
- **THEN** the control plane SHALL return all domains awaiting approval
- **AND THEN** this SHALL serve as the replacement for the former list-pending-activations endpoint

### Requirement: Describe Domain

Users SHALL be able to retrieve detailed information about a specific domain, including configuration, resources, and approval metadata. The `taskToken` field SHALL NOT be returned to domain owner callers. Platform team callers MAY access the `taskToken` when the domain status is `PENDING_APPROVAL` (authorization mechanism TBD).

#### Scenario: Describe existing domain

- **WHEN** a user calls the describe-domain API with a valid domain identifier they are authorized to access
- **THEN** the control plane SHALL return domain configuration (name, description, owner, ownerEmail, tags, status)
- **AND THEN** return resource identifiers for domain event buses, schema registry, dead letter queues, and monitoring artifacts when the domain is `ACTIVE`
- **AND THEN** return approval metadata (decidedBy, justification, approvedAt/rejectedAt) when available
- **AND THEN** the `taskToken` field SHALL NOT be included in the response for domain owner callers

#### Scenario: Describe unknown or unauthorized domain

- **WHEN** a user calls the describe-domain API with a domain identifier that does not exist or that they are not authorized to access
- **THEN** the control plane SHALL not leak domain existence details
- **AND THEN** return an error that represents either not-found or access-denied according to project security conventions

### Requirement: Decide Registration

Platform team members SHALL be able to approve or reject a domain registration request by submitting a decision via a synchronous API endpoint. The endpoint SHALL accept the task token, decision, decider identity, and justification. The Lambda SHALL call `SendTaskSuccess` or `SendTaskFailure` on the Step Functions workflow to resume the approval workflow. The Lambda SHALL NOT read from DynamoDB, emit events, or update domain status — these responsibilities belong to the approval workflow.

#### Scenario: Approval decision submitted successfully

- **WHEN** an authorized platform team member calls `POST /domains/registrations/decide` with a valid task token, decision of `ACCEPT`, `decidedBy` username, and optional justification
- **THEN** the decide Lambda SHALL call `sfn:SendTaskSuccess` with the task token and an output payload containing `approved: true`, `decidedBy`, and `justification`
- **AND THEN** the Lambda SHALL return `200 OK` with `{"message":"Decision recorded"}`

#### Scenario: Rejection decision submitted successfully

- **WHEN** an authorized platform team member calls `POST /domains/registrations/decide` with a valid task token, decision of `REJECT`, `decidedBy` username, and required justification
- **THEN** the decide Lambda SHALL call `sfn:SendTaskFailure` with the task token, error code `RegistrationRejected`, and cause containing `decidedBy` and `justification`
- **AND THEN** the Lambda SHALL return `200 OK` with `{"message":"Decision recorded"}`

#### Scenario: Decision with missing or invalid fields

- **WHEN** a caller submits a decision with missing required fields (taskToken, decision, decidedBy) or an invalid decision value
- **THEN** the Lambda SHALL return `400 Bad Request` with a descriptive validation error
- **AND THEN** no Step Functions callback SHALL be made

#### Scenario: Decision with invalid or expired task token

- **WHEN** a caller submits a decision with a task token that is expired, already used, or invalid
- **THEN** the `sfn:SendTaskSuccess` or `sfn:SendTaskFailure` call SHALL fail
- **AND THEN** the Lambda SHALL return `500 Internal Server Error` with a message indicating the callback failed

#### Scenario: Justification required on rejection

- **WHEN** a caller submits a decision of `REJECT` without a `justification` field
- **THEN** the Lambda SHALL return `400 Bad Request` indicating that justification is required for rejections

### Requirement: Update Domain

Domain owners SHALL be able to update mutable domain metadata by submitting a command that is processed asynchronously. The update command SHALL only accept `description` and `tags` fields. Status transitions SHALL NOT be performed via the update command. The API SHALL accept the command and return immediately; the outcome SHALL be communicated via an EventBridge event.

#### Scenario: Update command accepted and enqueued

- **WHEN** a domain owner calls the update-domain API with valid fields (description and/or tags)
- **THEN** API Gateway SHALL validate the request structure and enqueue the command to the domains SQS queue
- **AND THEN** the API SHALL return `202 Accepted` with `{"requestId":"<SQS-MessageId>","message":"Domain update request accepted"}`

#### Scenario: Update command processed and succeeded event emitted

- **WHEN** the domain command handler processes an update-domain message
- **THEN** the handler SHALL apply the changes to the domain record in DynamoDB
- **AND THEN** emit `UpdateDomainSucceeded` to the control plane event bus

#### Scenario: Update command fails

- **WHEN** the domain command handler processes an update-domain message and the update cannot be applied (e.g., domain not found, domain not in ACTIVE status)
- **THEN** the handler SHALL emit `UpdateDomainFailed` with the reason

### Requirement: Domain status model

The control plane SHALL use the following domain statuses to represent the lifecycle of a domain. Each status SHALL be unambiguous and indicate both the current state and whose responsibility it is to act next.

| Status | Meaning | Whose turn |
|--------|---------|------------|
| `PENDING_APPROVAL` | Registration submitted, awaiting human decision | Approver |
| `APPROVED` | Registration approved, provisioning not yet started | System |
| `REJECTED` | Registration rejected by approver | Terminal |
| `PROVISIONING` | Domain resources being created across accounts | System |
| `ACTIVE` | Domain operational, resources provisioned and ready | Domain owner |
| `PROVISIONING_FAILED` | Resource creation failed, operator intervention needed | Operator |
| `INACTIVE` | Domain disabled (semantics TBD, future workflow) | TBD |

#### Scenario: Valid status transitions

- **WHEN** a domain status transition occurs
- **THEN** only the following transitions SHALL be permitted:
  - `PENDING_APPROVAL` → `APPROVED` (approval workflow, approved path)
  - `PENDING_APPROVAL` → `REJECTED` (approval workflow, rejected path)
  - `APPROVED` → `PROVISIONING` (provisioning workflow, first step)
  - `PROVISIONING` → `ACTIVE` (provisioning workflow, success)
  - `PROVISIONING` → `PROVISIONING_FAILED` (provisioning workflow, failure)
  - `ACTIVE` → `INACTIVE` (future deactivation workflow, out of scope)

#### Scenario: Invalid status transition rejected

- **WHEN** any component attempts a status transition not listed in the valid transitions
- **THEN** the transition SHALL be rejected
- **AND THEN** the domain record SHALL remain in its current status
