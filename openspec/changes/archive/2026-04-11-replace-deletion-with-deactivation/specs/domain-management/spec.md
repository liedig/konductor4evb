## MODIFIED Requirements

### Requirement: Update Domain

Domain owners SHALL be able to update mutable domain metadata and configuration while preserving domain identity and enforcing governance. Domain owners SHALL also be able to deactivate a domain, which is a terminal, irreversible state transition that disables event routing while preserving all provisioned resources and audit history.

#### Scenario: Update domain metadata

- **WHEN** a domain owner calls the update-domain API with changes to description, tags, or contact information
- **THEN** the control plane SHALL validate that the caller is authorized to manage the domain
- **AND THEN** apply updates within a short, bounded time
- **AND THEN** make updated values visible via list-domains and describe-domain

#### Scenario: Suspend and resume domain

- **WHEN** a domain owner calls the update-domain API to suspend a domain that is currently ACTIVE
- **THEN** the control plane SHALL stop routing new events for that domain while preserving existing configuration and history
- **AND THEN** reflect the SUSPENDED status in domain metadata and metrics
- **AND WHEN** the owner calls the update-domain API to resume a domain that is currently SUSPENDED
- **THEN** the control plane SHALL resume event routing and update status to ACTIVE accordingly

#### Scenario: Deactivate an active domain

- **WHEN** a domain owner calls the update-domain API with `desiredStatus: DEACTIVATED` on a domain in ACTIVE status
- **THEN** the control plane SHALL validate that the caller is authorized to manage the domain
- **AND THEN** transition the domain status to DEACTIVATED
- **AND THEN** disable event routing on the domain's event buses (remove PutEvent permissions) so no new events are accepted
- **AND THEN** preserve all provisioned resources (event buses, schema registry, DLQs, dashboards) in place for audit and forensic purposes
- **AND THEN** emit a `DomainDeactivated` event to the control plane event bus

#### Scenario: Deactivate a suspended domain

- **WHEN** a domain owner calls the update-domain API with `desiredStatus: DEACTIVATED` on a domain in SUSPENDED status
- **THEN** the control plane SHALL transition the domain status to DEACTIVATED
- **AND THEN** preserve all provisioned resources in place
- **AND THEN** emit a `DomainDeactivated` event to the control plane event bus

#### Scenario: Deactivation is terminal and irreversible

- **WHEN** any caller attempts to change the status of a domain in DEACTIVATED state (e.g., to ACTIVE or SUSPENDED)
- **THEN** the control plane SHALL reject the request with a conflict error indicating that DEACTIVATED is a terminal state
- **AND THEN** the domain SHALL remain in DEACTIVATED status

#### Scenario: Deactivation from invalid state

- **WHEN** a domain owner calls the update-domain API with `desiredStatus: DEACTIVATED` on a domain that is not in ACTIVE or SUSPENDED status (e.g., PENDING_APPROVAL, CREATING, FAILED)
- **THEN** the control plane SHALL reject the request with a conflict error indicating the current state does not permit deactivation

#### Scenario: Attempt to modify immutable domain attributes

- **WHEN** a caller attempts to change immutable fields such as domain name or owner account
- **THEN** the control plane SHALL reject the update request
- **AND THEN** return a clear error that explains which fields cannot be modified

### Requirement: Domain management events

The domain management capability SHALL emit the following events. Each event SHALL have a well-defined type (or name), a consistent payload, and a defined emitter (control plane API layer or domain-creation workflow). Events SHALL be emitted on the event bus or channel designated for control plane domain lifecycle so that workflows and subscribers can react.

#### Events emitted by the control plane

| Event type | When emitted | Purpose | Minimum payload |
|------------|----------------|---------|------------------|
| `DomainRegistrationRequested` | After a register-domain request is accepted and persisted with status PendingApproval | Audit and optional approval-workflow triggers | `requestId`, `domainId` or `domainName`, `ownerAccountId`, `requestedAt`, `requestedBy` |
| `DomainRegistrationApproved` | After an authorized approver approves a request in PendingApproval state | Triggers the domain resource-creation workflow (e.g., Step Functions) | `requestId`, `domainId`, `domainName`, `ownerAccountId`, `approvedAt`, `approvedBy` |
| `DomainRegistrationRejected` | After an authorized approver rejects a request in PendingApproval state | Audit and notifications; no resource workflow triggered | `requestId`, `domainId`, `domainName`, `rejectedAt`, `rejectedBy`, optional `reason` |
| `DomainUpdated` | After a domain's mutable metadata or configuration is successfully updated (e.g., description, tags, suspend/resume) | Audit and downstream sync | `domainId`, `updatedAt`, `updatedBy`, optional summary of changed fields |
| `DomainDeactivated` | After a domain transitions to DEACTIVATED status | Audit, notifications, downstream cleanup of subscriptions and routing rules | `domainId`, `domainName`, `ownerAccountId`, `deactivatedAt`, `deactivatedBy`, `previousStatus` |

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

#### Scenario: Deactivation event emitted

- **WHEN** a domain transitions to DEACTIVATED status
- **THEN** the control plane SHALL emit a `DomainDeactivated` event with the minimum payload defined above
- **AND THEN** downstream consumers (e.g., subscription management, notification systems) SHALL be able to react to the deactivation

## REMOVED Requirements

### Requirement: Delete Domain

**Reason**: Domains are long-lived organizational constructs that accumulate audit history, provisioned resources, and cross-account relationships. Permanent deletion breaks audit trails and makes forensic investigation impossible. Deactivation replaces deletion to ensure domains are always recoverable and auditable.

**Migration**: Use the UpdateDomain API with `desiredStatus: DEACTIVATED` to disable a domain. Deactivated domains preserve all provisioned resources but disable event routing. The `DELETE /domains/{domainId}` endpoint is removed.
