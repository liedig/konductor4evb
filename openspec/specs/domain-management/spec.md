# Capability: Domain Management

## Requirements

### Requirement: Domain and request persistence

All domain details, lifecycle state, and approval metadata SHALL be stored in a single Amazon DynamoDB domains table. The control plane SHALL use DynamoDB as the system of record for domain metadata, resource identifiers, approval decisions, and the lifecycle status of each domain. There SHALL NOT be a separate registration requests table. The partition key SHALL be the domain name (`domainName`). The table SHALL have `by-owner` and `by-status` Global Secondary Indexes. There SHALL NOT be a `by-name` GSI (the partition key serves name-based lookups directly).

#### Scenario: Domain data stored in single table

- **WHEN** any domain is created, updated, approved, rejected, or provisioned
- **THEN** the control plane SHALL persist all domain details and status changes to the domains table
- **AND THEN** list and describe operations SHALL read from the domains table so that users see a consistent view of domains and their lifecycle state

#### Scenario: Domain record keyed by domain name

- **WHEN** a new domain is registered
- **THEN** the domain record SHALL be created with `domainName` as the partition key
- **AND THEN** no surrogate identifier (e.g., UUID) SHALL be generated or stored

#### Scenario: Approval-phase fields stored on domain record

- **WHEN** a domain registration enters the approval phase
- **THEN** the approval workflow SHALL write `taskToken`, `decidedBy`, `justification`, and decision timestamps directly to the domain record in the domains table
- **AND THEN** these fields SHALL coexist with the domain metadata fields on the same record

### Requirement: Domain management events

The domain management capability SHALL emit the following events. Each event SHALL have a well-defined type, a consistent payload, and a defined emitter (control plane command handler or workflow). Events SHALL be emitted on the control plane event bus. Command outcome events (`*Succeeded` / `*Failed`) are the primary mechanism by which callers learn the result of write operations. The approval workflow SHALL be the sole emitter of approval and rejection events. All event payloads SHALL identify the domain by `domainName` and SHALL NOT include a `domainId` field.

#### Events emitted by the control plane command handler

| Event type | When emitted | Purpose | Minimum payload |
|------------|--------------|---------|-----------------|
| `RegisterDomainSucceeded` | After the register-domain command is successfully processed | Confirms domain registration was accepted | `domainName`, `ownerAccountId`, `ownerEmail`, `environments`, `status`, `createdAt` |
| `RegisterDomainFailed` | After the register-domain command fails (e.g., duplicate name, validation error) | Surfaces processing failure to callers and audit | `domainName`, `ownerAccountId`, `reason`, `failedAt` |
| `UpdateDomainSucceeded` | After the update-domain command is successfully processed | Confirms domain metadata update was applied | `domainName`, `updatedAt`, `changedFields` |
| `UpdateDomainFailed` | After the update-domain command fails (e.g., domain not found) | Surfaces processing failure to callers and audit | `domainName`, `reason`, `failedAt` |

#### Events emitted by the approval workflow

| Event type | When emitted | Purpose | Minimum payload |
|------------|--------------|---------|-----------------|
| `DomainRegistrationApproved` | After the approval workflow processes an approval decision | Triggers the provisioning workflow | `domainName`, `ownerAccountId`, `ownerEmail`, `environments`, `approvedBy`, `justification`, `approvedAt` |
| `DomainRegistrationRejected` | After the approval workflow processes a rejection decision | Triggers rejection notification; audit | `domainName`, `ownerAccountId`, `ownerEmail`, `rejectedBy`, `justification`, `rejectedAt` |

#### Events emitted by the provisioning workflow

| Event type | When emitted | Purpose | Minimum payload |
|------------|--------------|---------|-----------------|
| `DomainProvisioningSucceeded` | When the outer provisioning workflow completes successfully across all environments | Control plane updates domain to ACTIVE with per-environment resource ARNs | `domainName`, `workflowExecutionId`, `completedAt`, `environments`, `resourceArns` (map keyed by env), `namespaceParameter`, `isNewAccount` |
| `DomainProvisioningFailed` | When the outer provisioning workflow fails | Control plane updates domain to PROVISIONING_FAILED | `domainName`, `workflowExecutionId`, `failedAt`, `failedEnvironment`, `failedStep`, `errorCode`, `errorMessage`, `compensationErrors` |

#### Scenario: Approval workflow is sole emitter of approval/rejection events

- **WHEN** a registration decision is submitted via the decide endpoint
- **THEN** the decide Lambda SHALL call `SendTaskSuccess` or `SendTaskFailure` on the Step Functions workflow
- **AND THEN** the approval workflow SHALL emit `DomainRegistrationApproved` or `DomainRegistrationRejected`
- **AND THEN** no other component SHALL emit approval or rejection events

#### Scenario: Approval forwards environments to provisioning

- **WHEN** the approval workflow emits `DomainRegistrationApproved`
- **THEN** the event payload SHALL include the `environments` list from the domain record
- **AND THEN** the provisioning workflow SHALL use this list as the target environments for the domain

#### Scenario: Command outcome events emitted for write operations

- **WHEN** a write command (register, update) is processed by the command handler
- **THEN** the handler SHALL emit either a `*Succeeded` or `*Failed` outcome event to the control plane bus
- **AND THEN** callers and downstream systems SHALL subscribe to these events to learn the result of their commands

### Requirement: Register Domain

Domain management is an administrative function of the control plane. Domain owners SHALL be able to request registration of a new domain by submitting a command that is processed asynchronously. The API SHALL accept the command and return immediately with a request ID (SQS message ID) and a descriptive message. Each request SHALL go through an approval process; only after approval SHALL the provisioning workflow be triggered. All domain details SHALL be stored in the single domains DynamoDB table. The domain name SHALL be used as the DynamoDB partition key. Uniqueness SHALL be enforced atomically via a conditional `PutItem` with `attribute_not_exists(domainName)` — the system SHALL NOT perform a pre-write query to check for duplicates.

The set of environments a domain is provisioned into SHALL be determined by the **platform team**, not the requester. The Nexus platform SHALL hold a single configured environment list (default `["dev", "staging", "prod"]`), and every approved domain SHALL be provisioned into the full list. The registration API SHALL NOT accept an `environments` field. All environments of a given domain SHALL be provisioned into the same `ownerAccountId`.

#### Scenario: Registration command accepted and enqueued

- **WHEN** an authorized domain owner calls the register-domain API with a valid domain name, owner account ID, owner email, and optional description/tags
- **THEN** API Gateway SHALL validate the request structure and enqueue the command to the domains SQS queue
- **AND THEN** the API SHALL return `202 Accepted` with `{"requestId":"<SQS-MessageId>","message":"Domain registration request accepted"}`

#### Scenario: Registration command processed and succeeded event emitted

- **WHEN** the domain command handler processes a register-domain message from the queue
- **THEN** the handler SHALL validate business rules (naming, reserved names)
- **AND THEN** read the platform-configured environment list
- **AND THEN** persist the domain in DynamoDB using a conditional `PutItem` with `domainName` as the partition key and a condition expression of `attribute_not_exists(domainName)` and status `PENDING_APPROVAL` and that environment list stamped onto the `environments` field of the domain record
- **AND THEN** start the approval Step Functions workflow
- **AND THEN** emit `RegisterDomainSucceeded` to the control plane event bus (including the stamped `environments` list)

#### Scenario: Registration command fails due to duplicate domain name

- **WHEN** the conditional `PutItem` fails with `ConditionalCheckFailedException` because a domain with the same name already exists
- **THEN** the handler SHALL emit `RegisterDomainFailed` to the control plane event bus with the reason "domain already exists"
- **AND THEN** NOT start any workflow

#### Scenario: Registration command fails business validation

- **WHEN** the domain command handler processes a register-domain message and a naming business rule is violated (e.g., reserved name, invalid format)
- **THEN** the handler SHALL emit `RegisterDomainFailed` to the control plane event bus with the reason
- **AND THEN** NOT persist any domain record or start any workflow

#### Scenario: Environment list is snapshotted at registration time

- **WHEN** a domain is registered
- **THEN** the environment list SHALL be captured from platform configuration at that moment and stored on the domain record
- **AND THEN** subsequent changes to the platform environment configuration SHALL NOT retroactively alter the `environments` field on existing domains

#### Scenario: Registration request rejected due to structural validation

- **WHEN** a caller invokes the register-domain API with missing required fields or invalid field formats
- **THEN** API Gateway SHALL reject the request synchronously with `400 Bad Request`
- **AND THEN** the malformed request SHALL NOT be enqueued

### Requirement: List Domains

Users SHALL be able to list the domains they have permission to access, with filters and pagination that scale to large organizations. The response SHALL identify each domain by `domainName` and SHALL NOT include a `domainId` field.

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

Users SHALL be able to retrieve detailed information about a specific domain by its domain name, including configuration, resources, and approval metadata. The API path SHALL be `GET /domains/{domainName}`. The `taskToken` field SHALL NOT be returned to domain owner callers. Platform team callers MAY access the `taskToken` when the domain status is `PENDING_APPROVAL`. Resource identifiers SHALL be returned as a map keyed by environment, where each entry contains the per-environment resource ARNs.

#### Scenario: Describe existing domain

- **WHEN** a user calls `GET /domains/{domainName}` with a valid domain name they are authorized to access
- **THEN** the control plane SHALL perform a `GetItem` on the domains table using `domainName` as the partition key
- **AND THEN** return domain configuration (name, description, owner, ownerEmail, tags, status, environments)
- **AND THEN** return resource identifiers as a map keyed by environment when the domain is `ACTIVE`; each entry SHALL contain `platformEventBusArn`, `appEventBusArn`, `schemaRegistryArn`, `subscriptionDlqArn`, `integrationDlqArn`, and `dashboardArn`
- **AND THEN** return domain-level identifiers (namespace parameter path, RAM share ARN) at the top level, not nested under any environment
- **AND THEN** return approval metadata (decidedBy, justification, approvedAt/rejectedAt) when available
- **AND THEN** the `taskToken` field SHALL NOT be included in the response for domain owner callers

#### Scenario: Describe unknown or unauthorized domain

- **WHEN** a user calls `GET /domains/{domainName}` with a domain name that does not exist or that they are not authorized to access
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

Domain owners SHALL be able to update mutable domain metadata by submitting a command that is processed asynchronously. The update command SHALL only accept `description` and `tags` fields. The domain name SHALL NOT be updatable. Status transitions SHALL NOT be performed via the update command. The API path SHALL be `PUT /domains/{domainName}`. The API SHALL accept the command and return immediately; the outcome SHALL be communicated via an EventBridge event.

#### Scenario: Update command accepted and enqueued

- **WHEN** a domain owner calls `PUT /domains/{domainName}` with valid fields (description and/or tags)
- **THEN** API Gateway SHALL validate the request structure and enqueue the command to the domains SQS queue
- **AND THEN** the `domainName` path parameter SHALL be injected into the message body by the API Gateway VTL mapping template
- **AND THEN** the API SHALL return `202 Accepted` with `{"requestId":"<SQS-MessageId>","message":"Domain update request accepted"}`

#### Scenario: Update command processed and succeeded event emitted

- **WHEN** the domain command handler processes an update-domain message
- **THEN** the handler SHALL apply the changes to the domain record in DynamoDB keyed by `domainName`
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

### Requirement: Domain name as primary identifier

The domain name SHALL be the sole identifier for a domain throughout the system. Domain names SHALL be globally unique within the Nexus installation. This uniqueness is an intentional governance feature that forces domain owners to make explicit bounded-context decisions, aligning with DDD and EDA principles. The system SHALL NOT generate or use a surrogate identifier (e.g., UUID) for domains. API paths, event payloads, DynamoDB keys, and Step Functions inputs SHALL all use the domain name as the domain identifier.

#### Scenario: Domain name used in API paths

- **WHEN** a caller interacts with the describe or update API
- **THEN** the API path SHALL use the domain name (e.g., `GET /domains/sales`) and SHALL NOT use a surrogate identifier

#### Scenario: Domain name used in event payloads

- **WHEN** the system emits a domain lifecycle event
- **THEN** the event payload SHALL identify the domain by `domainName` and SHALL NOT include a `domainId` field

#### Scenario: Domain name used as DynamoDB key

- **WHEN** a domain record is created or retrieved
- **THEN** the partition key SHALL be the domain name and SHALL NOT be a UUID

### Requirement: Account-per-domain design principle

The system SHALL assume one AWS account per domain for application-side resources. The trust stack is deployed per-account, and resource naming (`{org}-{env}-{domain}-*`) assumes no two domains share an account. The `ownerAccountId` field on the domain record represents the AWS account where this domain's application-side resources are provisioned. All environments of a given domain SHALL be provisioned into the same `ownerAccountId`.

#### Scenario: One domain per account

- **WHEN** a domain is registered with an `ownerAccountId`
- **THEN** all application-side resources for all environments of that domain SHALL be provisioned in that account
- **AND THEN** the trust stack in that account SHALL grant the Nexus cross-account access to manage those resources

### Requirement: Primary and secondary owner model

The domain record SHALL store a primary owner account (`ownerAccountId`). The system SHALL support a future secondary owner model where secondary owners are co-managers of the same AWS account, acting as backup decision-makers when the primary owner is unavailable. Resources always live in the primary owner's account. Secondary ownership is an authorization concern, not a data model concern — the partition key (`domainName`) is compatible with this future evolution.

#### Scenario: Primary owner is the resource host

- **WHEN** a domain is provisioned
- **THEN** all application-side resources SHALL be created in the primary owner's account
- **AND THEN** secondary owners (when implemented) SHALL NOT cause resources to be provisioned in additional accounts
