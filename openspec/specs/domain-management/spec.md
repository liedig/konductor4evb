# Capability: Domain Management

## Requirements

### Requirement: Domain and request persistence

All domain details and all domain registration requests and their status SHALL be stored in Amazon DynamoDB. The control plane SHALL use DynamoDB as the system of record for domain metadata, resource identifiers, and the lifecycle state of each registration request (e.g., pending approval, approved, creating, active, failed, rejected).

#### Scenario: Domain and request data stored in DynamoDB

- **WHEN** any domain registration request is created, updated, approved, or completed (success or failure)
- **THEN** the control plane SHALL persist the domain details and request status in DynamoDB
- **AND THEN** list and describe operations SHALL read from DynamoDB so that users see a consistent view of domains and pending activations

### Requirement: Domain management events

The domain management capability SHALL emit the following events. Each event SHALL have a well-defined type (or name), a consistent payload, and a defined emitter (control plane API layer or domain-creation workflow). Events SHALL be emitted on the event bus or channel designated for control plane domain lifecycle so that workflows and subscribers can react.

#### Events emitted by the control plane

| Event type | When emitted | Purpose | Minimum payload |
|------------|----------------|---------|------------------|
| `DomainRegistrationRequested` | After a register-domain request is accepted and persisted with status PendingApproval | Audit and optional approval-workflow triggers | `requestId`, `domainId` or `domainName`, `ownerAccountId`, `requestedAt`, `requestedBy` |
| `DomainRegistrationApproved` | After an authorized approver approves a request in PendingApproval state | Triggers the domain resource-creation workflow (e.g., Step Functions) | `requestId`, `domainId`, `domainName`, `ownerAccountId`, `approvedAt`, `approvedBy` |
| `DomainRegistrationRejected` | After an authorized approver rejects a request in PendingApproval state | Audit and notifications; no resource workflow triggered | `requestId`, `domainId`, `domainName`, `rejectedAt`, `rejectedBy`, optional `reason` |
| `DomainUpdated` | After a domain’s mutable metadata or configuration is successfully updated (e.g., description, tags, suspend/resume) | Audit and downstream sync | `domainId`, `updatedAt`, `updatedBy`, optional summary of changed fields |
| `DomainDeletionRequested` | After a delete-domain request is accepted and soft-delete or cleanup is initiated | Audit and cleanup workflows | `domainId`, `requestedAt`, `requestedBy`, optional `forceDelete` |
| `DomainDeleted` | After a domain is permanently removed (e.g., after retention window or cleanup completed) | Audit and downstream cleanup | `domainId`, `deletedAt` |

#### Events emitted by the domain-creation workflow

| Event type | When emitted | Purpose | Minimum payload |
|------------|----------------|---------|------------------|
| `DomainCreationStarted` | When the Step Functions (or equivalent) workflow begins execution for an approved request | Visibility and metrics | `requestId`, `domainId`, `workflowExecutionId`, `startedAt` |
| `DomainCreationSucceeded` | When the workflow completes successfully and all resources are provisioned | Control plane consumes this to set domain to Active and persist resource ARNs | `requestId`, `domainId`, `workflowExecutionId`, `completedAt`, `resourceArns` (event buses, schema registry, DLQ, etc.) |
| `DomainCreationFailed` | When the workflow fails (step failure, timeout, or AWS API error) | Control plane consumes this to set domain to failed and persist error detail | `requestId`, `domainId`, `workflowExecutionId`, `failedAt`, `errorCode`, `errorMessage`, optional `failedStep` |

#### Scenario: Control plane emits approval event and workflow is triggered

- **WHEN** a registration request is approved
- **THEN** the control plane SHALL emit `DomainRegistrationApproved` with the minimum payload above
- **AND THEN** the domain-creation workflow (e.g., Step Functions) SHALL be invoked in response to this event so that resource provisioning runs asynchronously

#### Scenario: Workflow emits completion events consumed by control plane

- **WHEN** the domain-creation workflow completes (success or failure)
- **THEN** the workflow SHALL emit `DomainCreationSucceeded` or `DomainCreationFailed` with the minimum payload above
- **AND THEN** the control plane SHALL consume that event and update the domain record in DynamoDB accordingly (Active with resource ARNs, or failed with error details)

### Requirement: Register Domain

Domain management is an administrative function of the control plane. The control plane SHALL assume it already has the necessary permissions to create cross-account resources. Platform teams SHALL be able to request registration of a new domain. Each request SHALL go through an approval process; only after approval SHALL the resource-creation workflow be triggered. All domain details and request state SHALL be stored in DynamoDB.

#### Scenario: Registration request submitted and stored

- **WHEN** an authorized administrator calls the register-domain API with a valid domain name, description, owner account, and optional tags
- **THEN** the control plane SHALL validate the request against naming and uniqueness rules
- **AND THEN** persist the registration request in DynamoDB with a status indicating it is pending approval (e.g., PendingApproval) and return a domain identifier or request identifier and that status
- **AND THEN** not emit any workflow-triggering event until the request has been approved

#### Scenario: Registration request approved and workflow initiated

- **WHEN** a registration request in pending-approval state is approved by an authorized approver
- **THEN** the control plane SHALL update the request record in DynamoDB to a status that indicates provisioning has started (e.g., Approved or Creating)
- **AND THEN** emit an event that initiates the domain-creation workflow (e.g., an AWS Step Functions workflow) so that provisioning of event buses, schema registry, dead letter queue, IAM and resource policies, and monitoring is performed asynchronously

#### Scenario: Registration request rejected during approval

- **WHEN** a registration request in pending-approval state is rejected by an authorized approver
- **THEN** the control plane SHALL update the request record in DynamoDB to a rejected status
- **AND THEN** not emit any workflow-triggering event
- **AND THEN** the domain SHALL remain non-active and list-pending-domain-activations (and describe) SHALL reflect the rejected status

#### Scenario: Registration request rejected due to validation

- **WHEN** a caller invokes the register-domain API with an invalid name, duplicate name, or malformed metadata
- **THEN** the control plane SHALL reject the request synchronously
- **AND THEN** persist no record in DynamoDB and emit no workflow-triggering event
- **AND THEN** return a clear error that identifies which fields are invalid

#### Scenario: Caller not authorized to register domains

- **WHEN** a caller who lacks permission to perform domain management operations invokes the register-domain API
- **THEN** the control plane SHALL reject the request
- **AND THEN** return an access-denied error without emitting any event or creating any state

#### Scenario: Workflow completes successfully

- **WHEN** the domain-creation workflow (e.g., Step Functions) completes successfully
- **THEN** the workflow SHALL emit a success event that the control plane consumes
- **AND THEN** the control plane SHALL update the domain record in DynamoDB to Active and attach resource identifiers (event buses, schema registry, DLQ, dashboards, policies)
- **AND THEN** describe-domain and list-domains SHALL reflect the domain as Active with full metadata and resource ARNs

#### Scenario: Workflow fails

- **WHEN** the domain-creation workflow fails (e.g., a step times out or an AWS API call fails)
- **THEN** the workflow SHALL emit a failure event that the control plane consumes
- **AND THEN** the control plane SHALL update the domain record in DynamoDB to a failed state and retain enough detail (e.g., error code, message, step) for operators to diagnose and retry or abandon
- **AND THEN** describe-domain (or a dedicated get-operation API) SHALL expose the failure reason and current status so that administrators can correct configuration and retry registration if applicable

#### Scenario: Long-running provisioning and status visibility

- **WHEN** the domain-creation workflow runs longer than the target SLO (e.g., 5 minutes for 95% of cases)
- **THEN** the control plane SHALL continue the workflow in the background
- **AND THEN** expose the current status (e.g., Creating) and, when available, workflow execution identifiers via describe-domain or a dedicated operation-status API
- **AND THEN** emit metrics and logs so that platform teams can detect and investigate slow provisioning

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

Domain owners SHALL be able to update mutable domain metadata and configuration while preserving domain identity and enforcing governance.

#### Scenario: Update domain metadata

- **WHEN** a domain owner calls the update-domain API with changes to description, tags, or contact information
- **THEN** the control plane SHALL validate that the caller is authorized to manage the domain
- **AND THEN** apply updates within a short, bounded time
- **AND THEN** make updated values visible via list-domains and describe-domain

#### Scenario: Suspend and resume domain

- **WHEN** a domain owner calls the update-domain API to disable or suspend a domain
- **THEN** the control plane SHALL stop routing new events for that domain while preserving existing configuration and history
- **AND THEN** reflect the suspended status in domain metadata and metrics
- **AND WHEN** the owner re-enables the domain
- **THEN** the control plane SHALL resume event routing and update status and metrics accordingly

#### Scenario: Attempt to modify immutable domain attributes

- **WHEN** a caller attempts to change immutable fields such as domain name or owner account
- **THEN** the control plane SHALL reject the update request
- **AND THEN** return a clear error that explains which fields cannot be modified

### Requirement: Delete Domain

Domain owners SHALL be able to delete domains only when there are no dependencies or resources associated with the services used by that domain. Deletion SHALL be blocked until all such dependencies and resources are removed or released; governance may allow a force-delete path in exceptional cases. Data retention and soft-delete behavior SHALL apply when deletion is permitted.

#### Scenario: Delete domain when no dependencies or associated resources exist

- **WHEN** a domain owner calls the delete-domain API and the domain has no dependencies or resources associated with the services used (e.g., no active subscriptions, no rules or targets still referencing the domain, no outstanding resources in EventBridge, schema registry, or other integrated services)
- **THEN** the control plane SHALL validate ownership and preconditions
- **AND THEN** perform a controlled cleanup of domain resources including event buses, schema registry, dead letter queues, monitoring artifacts, and IAM or resource policies
- **AND THEN** retain necessary logs and DLQ messages for the configured retention period

#### Scenario: Prevent deletion when dependencies or associated resources exist

- **WHEN** a domain owner calls the delete-domain API and the domain has one or more dependencies or resources still associated with the services used (e.g., active subscriptions, rules, targets, or other resources in EventBridge or related services)
- **THEN** the control plane SHALL reject the request by default
- **AND THEN** return a clear error that identifies the blocking dependencies or resources (e.g., subscription count, resource types) so that the owner can remove them and retry
- **AND THEN** allow deletion only after those dependencies or resources have been removed, unless governance explicitly permits a force-delete path

#### Scenario: Soft delete and recovery window

- **WHEN** a domain is deleted according to governance rules
- **THEN** the control plane SHALL treat the domain as soft-deleted for a defined retention window
- **AND THEN** prevent new publish or subscribe operations during this window
- **AND THEN** allow recovery of the domain configuration and resources within the window if supported by the implementation
- **AND THEN** permanently remove domain configuration and resources after the window has expired
