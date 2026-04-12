# Capability: Domain Management

## Requirements

### Requirement: Domain and request persistence

All domain details and all domain registration requests and their status SHALL be stored in Amazon DynamoDB. The control plane SHALL use DynamoDB as the system of record for domain metadata, resource identifiers, and the lifecycle state of each registration request (e.g., pending approval, approved, creating, active, failed, rejected).

#### Scenario: Domain and request data stored in DynamoDB

- **WHEN** any domain registration request is created, updated, approved, or completed (success or failure)
- **THEN** the control plane SHALL persist the domain details and request status in DynamoDB
- **AND THEN** list and describe operations SHALL read from DynamoDB so that users see a consistent view of domains and pending activations

### Requirement: Domain management events

The domain management capability SHALL emit the following events. Each event SHALL have a well-defined type (or name), a consistent payload, and a defined emitter (control plane command handler or domain-creation workflow). Events SHALL be emitted on the event bus or channel designated for control plane domain lifecycle so that workflows and subscribers can react. Command outcome events (`*Succeeded` / `*Failed`) are the primary mechanism by which callers learn the result of write operations.

#### Events emitted by the control plane

| Event type | When emitted | Purpose | Minimum payload |
|------------|----------------|---------|------------------|
| `RegisterDomainSucceeded` | After the register-domain command is successfully processed | Confirms domain registration was accepted; callers subscribe to learn the outcome | `requestId`, `domainId`, `domainName`, `ownerAccountId`, `status`, `createdAt` |
| `RegisterDomainFailed` | After the register-domain command fails (e.g., duplicate name, validation error) | Surfaces processing failure to callers and audit | `domainName`, `ownerAccountId`, `reason`, `failedAt` |
| `DomainRegistrationApproved` | After an authorized approver approves a request in PendingApproval state | Triggers the domain resource-creation workflow | `requestId`, `domainId`, `domainName`, `ownerAccountId`, `approvedAt`, `approvedBy` |
| `DomainRegistrationRejected` | After an authorized approver rejects a request in PendingApproval state | Audit and notifications; no resource workflow triggered | `requestId`, `domainId`, `domainName`, `rejectedAt`, `rejectedBy`, optional `reason` |
| `UpdateDomainSucceeded` | After the update-domain command is successfully processed | Confirms domain update was applied | `domainId`, `updatedAt`, `changedFields` |
| `UpdateDomainFailed` | After the update-domain command fails (e.g., domain not found, invalid state transition) | Surfaces processing failure to callers and audit | `domainId`, `reason`, `failedAt` |
| `DomainDeactivated` | After a domain transitions to DEACTIVATED status | Audit, notifications, downstream cleanup | `domainId`, `domainName`, `ownerAccountId`, `deactivatedAt`, `deactivatedBy`, `previousStatus` |
| `DecideRegistrationSucceeded` | After the decide-registration command is successfully processed | Confirms decision was recorded and Step Functions callback sent | `requestId`, `decision`, `decidedAt` |
| `DecideRegistrationFailed` | After the decide-registration command fails (e.g., request not found, expired task token) | Surfaces processing failure to callers and audit | `requestId`, `reason`, `failedAt` |

#### Events emitted by the domain-creation workflow

| Event type | When emitted | Purpose | Minimum payload |
|------------|----------------|---------|------------------|
| `DomainCreationStarted` | When the Step Functions workflow begins execution for an approved request | Visibility and metrics | `requestId`, `domainId`, `workflowExecutionId`, `startedAt` |
| `DomainCreationSucceeded` | When the workflow completes successfully and all resources are provisioned | Control plane consumes this to set domain to Active and persist resource ARNs | `requestId`, `domainId`, `workflowExecutionId`, `completedAt`, `resourceArns` |
| `DomainCreationFailed` | When the workflow fails | Control plane consumes this to set domain to failed with error detail | `requestId`, `domainId`, `workflowExecutionId`, `failedAt`, `errorCode`, `errorMessage`, optional `failedStep` |

#### Scenario: Control plane emits approval event and workflow is triggered

- **WHEN** a registration request is approved
- **THEN** the control plane SHALL emit `DomainRegistrationApproved` with the minimum payload above
- **AND THEN** the domain-creation workflow SHALL be invoked in response to this event so that provisioning runs asynchronously

#### Scenario: Workflow emits completion events consumed by control plane

- **WHEN** the domain-creation workflow completes (success or failure)
- **THEN** the workflow SHALL emit `DomainCreationSucceeded` or `DomainCreationFailed` with the minimum payload above
- **AND THEN** the control plane SHALL consume that event and update the domain record in DynamoDB accordingly

#### Scenario: Deactivation event emitted

- **WHEN** a domain transitions to DEACTIVATED status
- **THEN** the control plane SHALL emit a `DomainDeactivated` event with the minimum payload defined above
- **AND THEN** downstream consumers SHALL be able to react to the deactivation

#### Scenario: Command outcome events emitted for write operations

- **WHEN** a write command (register, update, decide) is processed by the command handler
- **THEN** the handler SHALL emit either a `*Succeeded` or `*Failed` outcome event to the control plane bus
- **AND THEN** callers and downstream systems SHALL subscribe to these events to learn the result of their commands

### Requirement: Register Domain

Domain management is an administrative function of the control plane. Platform teams SHALL be able to request registration of a new domain by submitting a command that is processed asynchronously. The API SHALL accept the command and return immediately; the outcome SHALL be communicated via an EventBridge event. Each request SHALL go through an approval process; only after approval SHALL the resource-creation workflow be triggered. All domain details and request state SHALL be stored in DynamoDB.

#### Scenario: Registration command accepted and enqueued

- **WHEN** an authorized administrator calls the register-domain API with a valid domain name, description, owner account, and optional tags
- **THEN** API Gateway SHALL validate the request structure and enqueue the command to the domains SQS queue
- **AND THEN** the API SHALL return `202 Accepted` with `{"message":"OK"}`
- **AND THEN** the caller SHALL NOT receive a synchronous domain ID or request ID in the response

#### Scenario: Registration command processed and succeeded event emitted

- **WHEN** the domain command handler processes a register-domain message from the queue
- **THEN** the handler SHALL validate business rules (naming, uniqueness)
- **AND THEN** persist the registration request in DynamoDB with status PendingApproval
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

#### Scenario: Registration request approved and workflow initiated

- **WHEN** a registration request in pending-approval state is approved by an authorized approver
- **THEN** the control plane SHALL update the request record in DynamoDB and emit `DomainRegistrationApproved`
- **AND THEN** the domain-creation workflow SHALL be triggered

#### Scenario: Registration request rejected during approval

- **WHEN** a registration request in pending-approval state is rejected
- **THEN** the control plane SHALL update the request record to rejected status
- **AND THEN** emit `DomainRegistrationRejected`

#### Scenario: Workflow completes successfully

- **WHEN** the domain-creation workflow completes successfully
- **THEN** the workflow SHALL emit `DomainCreationSucceeded`
- **AND THEN** the control plane SHALL update the domain record to Active with resource ARNs

#### Scenario: Workflow fails

- **WHEN** the domain-creation workflow fails
- **THEN** the workflow SHALL emit `DomainCreationFailed`
- **AND THEN** the control plane SHALL update the domain record to a failed state with error details

### Requirement: List Domains

Users SHALL be able to list the domains they have permission to access, with filters and pagination that scale to large organizations.

#### Scenario: List domains with default filters

- **WHEN** a user calls the list-domains API with no filters
- **THEN** the control plane SHALL return all domains the caller is authorized to see
- **AND THEN** include basic metadata such as name, description, owner, creation date, and status for each domain
- **AND THEN** return results in a paginated form that can scale to thousands of domains

#### Scenario: List domains with filters

- **WHEN** a user calls the list-domains API with filters such as owner account, tags, or status
- **THEN** the control plane SHALL apply those filters server-side
- **AND THEN** return only matching domains along with pagination tokens where needed

### Requirement: List pending domain activations

Authorized users SHALL be able to list domain registration requests and their status so that pending approvals and in-progress activations are visible. Request and status data SHALL be read from DynamoDB.

#### Scenario: List all pending domain activations

- **WHEN** an authorized user calls the list-pending-domain-activations API (or equivalent list with a filter for non-active requests)
- **THEN** the control plane SHALL return all domain registration requests that are not yet active (e.g., PendingApproval, Approved, Creating, Failed, Rejected) that the caller is authorized to see
- **AND THEN** include for each request at least: request or domain identifier, status, domain name, owner account, and timestamp of last status change
- **AND THEN** support pagination so that large numbers of pending requests can be browsed

#### Scenario: List pending domain activations filtered by status

- **WHEN** an authorized user requests a list of pending domain activations filtered by status (e.g., only PendingApproval or only Creating)
- **THEN** the control plane SHALL return only registration requests in the requested status or statuses
- **AND THEN** return results from DynamoDB with pagination as needed

### Requirement: Describe Domain

Users SHALL be able to retrieve detailed information about a specific domain, including configuration, resources, and high-level usage statistics.

#### Scenario: Describe existing domain

- **WHEN** a user calls the describe-domain API with a valid domain identifier they are authorized to access
- **THEN** the control plane SHALL return domain configuration (name, description, owner, tags, status)
- **AND THEN** return resource identifiers for domain event buses, schema registry, dead letter queues, and monitoring artifacts
- **AND THEN** return high-level statistics such as event counts, subscription counts, and schema counts, updated within an acceptable freshness window

#### Scenario: Describe unknown or unauthorized domain

- **WHEN** a user calls the describe-domain API with a domain identifier that does not exist or that they are not authorized to access
- **THEN** the control plane SHALL not leak domain existence details
- **AND THEN** return an error that represents either not-found or access-denied according to project security conventions

### Requirement: Update Domain

Domain owners SHALL be able to update mutable domain metadata and configuration, including deactivation, by submitting a command that is processed asynchronously. The API SHALL accept the command and return immediately; the outcome SHALL be communicated via an EventBridge event.

#### Scenario: Update command accepted and enqueued

- **WHEN** a domain owner calls the update-domain API with valid fields
- **THEN** API Gateway SHALL validate the request structure and enqueue the command to the domains SQS queue
- **AND THEN** the API SHALL return `202 Accepted` with `{"message":"OK"}`

#### Scenario: Update command processed and succeeded event emitted

- **WHEN** the domain command handler processes an update-domain message
- **THEN** the handler SHALL apply the changes to DynamoDB
- **AND THEN** emit `UpdateDomainSucceeded` to the control plane event bus

#### Scenario: Update command fails

- **WHEN** the domain command handler processes an update-domain message and the update cannot be applied (e.g., domain not found, invalid state transition)
- **THEN** the handler SHALL emit `UpdateDomainFailed` with the reason

#### Scenario: Deactivate domain via update command

- **WHEN** a domain owner sends an update command with `desiredStatus: DEACTIVATED`
- **THEN** the handler SHALL transition the domain to DEACTIVATED, remove PutEvent permissions, and emit `DomainDeactivated`
- **AND THEN** also emit `UpdateDomainSucceeded`

#### Scenario: Attempt to modify immutable domain attributes

- **WHEN** a caller attempts to change immutable fields such as domain name or owner account via structural validation at API Gateway
- **THEN** if the schema enforces this, API Gateway SHALL reject with `400 Bad Request`
- **AND THEN** the command SHALL NOT be enqueued
