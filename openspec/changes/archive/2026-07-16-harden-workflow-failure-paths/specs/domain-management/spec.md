# Capability: Domain Management (delta)

## MODIFIED Requirements

### Requirement: Domain status model

The control plane SHALL use the following domain statuses to represent the lifecycle of a domain. Each status SHALL be unambiguous and indicate both the current state and whose responsibility it is to act next.

| Status | Meaning | Whose turn |
|--------|---------|------------|
| `PENDING_APPROVAL` | Registration submitted, awaiting human decision | Approver |
| `APPROVED` | Registration approved, provisioning not yet started | System |
| `REJECTED` | Registration rejected by approver (or expired unanswered) | Terminal |
| `APPROVAL_NOTIFICATION_FAILED` | Registration accepted but the approval request could not be delivered; no decision was made | Operator |
| `PROVISIONING` | Domain resources being created across accounts | System |
| `ACTIVE` | Domain operational, resources provisioned and ready | Domain owner |
| `PROVISIONING_FAILED` | Resource creation failed, operator intervention needed | Operator |
| `INACTIVE` | Domain disabled (semantics TBD, future workflow) | TBD |

#### Scenario: Valid status transitions

- **WHEN** a domain status transition occurs
- **THEN** only the following transitions SHALL be permitted:
  - `PENDING_APPROVAL` → `APPROVED` (approval workflow, approved path)
  - `PENDING_APPROVAL` → `REJECTED` (approval workflow, rejected or timeout path)
  - `PENDING_APPROVAL` → `APPROVAL_NOTIFICATION_FAILED` (approval workflow, notification-failure path)
  - `APPROVED` → `PROVISIONING` (provisioning workflow, first step)
  - `PROVISIONING` → `ACTIVE` (provisioning workflow, success)
  - `PROVISIONING` → `PROVISIONING_FAILED` (provisioning workflow, failure)
  - `ACTIVE` → `INACTIVE` (future deactivation workflow, out of scope)

#### Scenario: Invalid status transition rejected

- **WHEN** any component attempts a status transition not listed in the valid transitions
- **THEN** the transition SHALL be rejected
- **AND THEN** the domain record SHALL remain in its current status

#### Scenario: Notification failure is not a rejection

- **WHEN** the approval workflow cannot deliver the approval request notification
- **THEN** the domain SHALL transition to `APPROVAL_NOTIFICATION_FAILED`, not `REJECTED`
- **AND THEN** the record SHALL indicate operator intervention is required (recovery is operator-driven; no automated transition out of this status is defined)

### Requirement: Domain management events

The domain management capability SHALL emit the following events. Each event SHALL have a well-defined type, a consistent payload, and a defined emitter (control plane command handler or workflow). Events SHALL be emitted on the control plane event bus. Command outcome events (`*Succeeded` / `*Failed`) are the primary mechanism by which callers learn the result of write operations. The approval workflow SHALL be the sole emitter of approval, rejection, and approval-notification-failure events. All event payloads SHALL identify the domain by `domainName` and SHALL NOT include a `domainId` field. Events that carry domain owner information SHALL include `secondaryOwnerEmails` when present on the domain record.

#### Events emitted by the control plane command handler

| Event type | When emitted | Purpose | Minimum payload |
|------------|--------------|---------|-----------------|
| `RegisterDomainSucceeded` | After the register-domain command is successfully processed | Confirms domain registration was accepted | `domainName`, `ownerAccountId`, `ownerEmail`, `secondaryOwnerEmails`, `environments`, `status`, `createdAt` |
| `RegisterDomainFailed` | After the register-domain command fails (e.g., duplicate name, validation error) | Surfaces processing failure to callers and audit | `domainName`, `ownerAccountId`, `reason`, `failedAt` |
| `UpdateDomainSucceeded` | After the update-domain command is successfully processed | Confirms domain metadata update was applied | `domainName`, `updatedAt`, `changedFields` |
| `UpdateDomainFailed` | After the update-domain command fails (e.g., domain not found) | Surfaces processing failure to callers and audit | `domainName`, `reason`, `failedAt` |

#### Events emitted by the approval workflow

| Event type | When emitted | Purpose | Minimum payload |
|------------|--------------|---------|-----------------|
| `DomainRegistrationApproved` | After the approval workflow processes an approval decision | Triggers the provisioning workflow | `domainName`, `ownerAccountId`, `ownerEmail`, `secondaryOwnerEmails`, `environments`, `approvedBy`, `justification`, `approvedAt` |
| `DomainRegistrationRejected` | After the approval workflow processes a rejection decision or the approval wait times out | Triggers rejection notification; audit | `domainName`, `ownerAccountId`, `ownerEmail`, `secondaryOwnerEmails`, `rejectedBy`, `justification`, `rejectedAt` |
| `DomainApprovalNotificationFailed` | After the approval workflow exhausts retries delivering the approval request | Operator alerting; audit | `domainName`, `errorMessage`, `failedAt` |

#### Events emitted by the provisioning workflow

| Event type | When emitted | Purpose | Minimum payload |
|------------|--------------|---------|-----------------|
| `DomainProvisioningSucceeded` | When the outer provisioning workflow completes successfully across all environments | Control plane updates domain to ACTIVE with per-environment resource ARNs | `domainName`, `workflowExecutionId`, `completedAt`, `environments`, `resourceArns` (map keyed by env), `namespaceParameter`, `isNewAccount` |
| `DomainProvisioningFailed` | When the outer provisioning workflow fails | Control plane updates domain to PROVISIONING_FAILED | `domainName`, `workflowExecutionId`, `failedAt`, `failedEnvironment`, `failedStep`, `errorCode`, `errorMessage`, `compensationErrors` (empty array when no compensation errors occurred) |

#### Scenario: Approval workflow is sole emitter of approval/rejection events

- **WHEN** a registration decision is submitted via the decide endpoint
- **THEN** the decide Lambda SHALL call `SendTaskSuccess` or `SendTaskFailure` on the Step Functions workflow
- **AND THEN** the approval workflow SHALL emit `DomainRegistrationApproved` or `DomainRegistrationRejected`
- **AND THEN** no other component SHALL emit approval, rejection, or approval-notification-failure events

#### Scenario: Approval forwards environments to provisioning

- **WHEN** the approval workflow emits `DomainRegistrationApproved`
- **THEN** the event payload SHALL include the `environments` list from the domain record
- **AND THEN** the provisioning workflow SHALL use this list as the target environments for the domain

#### Scenario: Command outcome events emitted for write operations

- **WHEN** a write command (register, update) is processed by the command handler
- **THEN** the handler SHALL emit either a `*Succeeded` or `*Failed` outcome event to the control plane bus
- **AND THEN** callers and downstream systems SHALL subscribe to these events to learn the result of their commands

#### Scenario: Notification failure event is operator-facing

- **WHEN** `DomainApprovalNotificationFailed` is emitted
- **THEN** no domain-owner-facing email notification SHALL be triggered by it (email delivery is the failed dependency)
- **AND THEN** the event SHALL be available on the control-plane bus for operator alerting and audit
