# Spec Delta

## ADDED Requirements

### Requirement: Per-environment account overrides

Registration requests MAY carry an optional `environmentAccounts` map that overrides the account individual environments are provisioned into. `ownerAccountId` remains the default account for every environment. Map keys SHALL be members of the platform's configured environment list; map values SHALL be 12-digit AWS account IDs and need not be unique — multiple environments MAY resolve to the same account. The registration API SHALL continue to reject any `environments` field: the mapping selects accounts, never environments. The effective account for an environment SHALL resolve as the `environmentAccounts` value for that environment when present, otherwise `ownerAccountId`. The map SHALL be recorded on the domain record exactly as submitted (sparse) and SHALL NOT be mutable after registration. Trust-role reachability for a mapped account SHALL NOT be checked at request time; an unreachable account surfaces as a per-environment provisioning failure, exactly as an unreachable `ownerAccountId` does today.

#### Scenario: Default resolution without overrides

- **WHEN** a domain is registered without an `environmentAccounts` field
- **THEN** every environment SHALL resolve to `ownerAccountId`
- **AND THEN** the domain record SHALL store no map attribute (identical in shape to a registration submitted before this capability existed), while emitted events SHALL carry the resolved map as an additive field alongside the pre-existing ones

#### Scenario: Sparse override selects an account for one environment

- **WHEN** a domain is registered with `ownerAccountId: "111122223333"` and `environmentAccounts: {"prod": "444455556666"}` on a platform configured with `["dev", "staging", "prod"]`
- **THEN** `dev` and `staging` SHALL resolve to `111122223333` and `prod` SHALL resolve to `444455556666`
- **AND THEN** the domain record SHALL store the map exactly as submitted (`{"prod": "444455556666"}`)

#### Scenario: Duplicate accounts across environments are accepted

- **WHEN** a registration maps two or more environments to the same account ID
- **THEN** the request SHALL be accepted, because sharing one account across environments is a supported topology

#### Scenario: Override key outside the platform environment list is rejected

- **WHEN** a registration carries an `environmentAccounts` key that is not a member of the platform's configured environment list
- **THEN** the command SHALL fail validation and emit `RegisterDomainFailed` with the offending key named in the reason
- **AND THEN** no domain record SHALL be persisted and no workflow SHALL be started

#### Scenario: Malformed account value is rejected

- **WHEN** a registration carries an `environmentAccounts` value that is not a 12-digit AWS account ID
- **THEN** the command SHALL fail validation and emit `RegisterDomainFailed` with the reason
- **AND THEN** no domain record SHALL be persisted and no workflow SHALL be started

#### Scenario: The mapping is immutable after registration

- **WHEN** a caller submits `PUT /domains/{domainName}` carrying an `environmentAccounts` field
- **THEN** the field SHALL NOT be applied to the domain record
- **AND THEN** changing an environment's account SHALL require deprovisioning and re-provisioning that environment (a process deferred to a future change)

## MODIFIED Requirements

### Requirement: Register Domain

Domain management is an administrative function of the control plane. Any authenticated OAuth caller who belongs to at least one team group SHALL be able to request registration of a new domain by submitting a command that is processed asynchronously; IAM callers SHALL remain able to register domains exactly as before. The API SHALL accept the command and return immediately with a request ID (SQS message ID) and a descriptive message. Each request SHALL go through an approval process; only after approval SHALL the provisioning workflow be triggered. All domain details SHALL be stored in the single domains DynamoDB table. The domain name SHALL be used as the DynamoDB partition key. Uniqueness SHALL be enforced atomically via a conditional `PutItem` with `attribute_not_exists(domainName) OR #status = :REJECTED` — the system SHALL NOT perform a pre-write query to check for duplicates. A `REJECTED` record SHALL NOT block its domain name: a new registration for that name SHALL atomically replace the rejected record with a fresh registration (new `requestId`, status `PENDING_APPROVAL`, fresh approval cycle); rejection history remains available through the `DomainRegistrationRejected` events on the control plane bus.

For OAuth callers the registration request SHALL carry an `owningTeam` naming exactly one of the caller's Cognito team groups, which SHALL be recorded on the domain record and used to write the owner binding at approval (see domain-ownership). IAM callers MAY omit `owningTeam`; when omitted, no owner binding is created at approval and ownership administration falls to the platform team.

The set of environments a domain is provisioned into SHALL be determined by the **platform team**, not the requester. The Konductor platform SHALL hold a single configured environment list (default `["dev", "staging", "prod"]`), and every approved domain SHALL be provisioned into the full list. The registration API SHALL NOT accept an `environments` field. Every environment of a domain SHALL be provisioned into the account resolved for that environment — `ownerAccountId` by default, or the account named by an `environmentAccounts` override (see per-environment account overrides).

The registration request SHALL include at least one secondary owner email address. Secondary owners are co-managers of the domain who serve as backup contacts when the primary owner is unavailable. At this stage, secondary owner emails are captured for informational purposes only — no notifications are sent to secondary owners.

#### Scenario: Registration command accepted and enqueued

- **WHEN** an authorized domain owner calls the register-domain API with a valid domain name, owner account ID, owner email, at least one secondary owner email, and optional description/tags
- **THEN** API Gateway SHALL validate the request structure (including `secondaryOwnerEmails` with `minItems: 1`) and enqueue the command to the domains SQS queue
- **AND THEN** the API SHALL return `202 Accepted` with `{"requestId":"<SQS-MessageId>","message":"Domain registration request accepted"}`

#### Scenario: OAuth registration carries the owning team

- **WHEN** an OAuth caller submits a register-domain command with `owningTeam` set to one of the team groups in their token
- **THEN** the command handler SHALL record `owningTeam` on the domain record
- **AND THEN** the approval flow SHALL use it to create the owner binding on acceptance

#### Scenario: OAuth registration without a matching team is denied

- **WHEN** an OAuth caller submits a register-domain command with an `owningTeam` that is absent from their token's team groups, or omits `owningTeam`
- **THEN** the command SHALL be denied with `AccessDenied`
- **AND THEN** no domain record SHALL be persisted and no workflow SHALL be started

#### Scenario: Registration command processed and succeeded event emitted

- **WHEN** the domain command handler processes a register-domain message from the queue
- **THEN** the handler SHALL validate business rules (naming, reserved names, and any `environmentAccounts` contract rules)
- **AND THEN** read the platform-configured environment list
- **AND THEN** persist the domain in DynamoDB using a conditional `PutItem` with `domainName` as the partition key and a condition expression of `attribute_not_exists(domainName) OR #status = :REJECTED` and status `PENDING_APPROVAL` and that environment list stamped onto the `environments` field of the domain record, any `environmentAccounts` map stored exactly as submitted, and `secondaryOwnerEmails` stored as a String Set
- **AND THEN** start the approval Step Functions workflow
- **AND THEN** emit `RegisterDomainSucceeded` to the control plane event bus (including the stamped `environments` list, the effective `environmentAccounts` map, and `secondaryOwnerEmails`)

#### Scenario: Re-registration after rejection replaces the rejected record

- **WHEN** the domain command handler processes a register-domain message for a domain name whose existing record has status `REJECTED`
- **THEN** the conditional `PutItem` SHALL succeed, replacing the rejected record wholesale with the fresh registration (new `requestId`, new owner details, status `PENDING_APPROVAL`)
- **AND THEN** a new approval workflow SHALL start and `RegisterDomainSucceeded` SHALL be emitted
- **AND THEN** the replaced record's rejection fields (`decidedBy`, `justification`, `rejectedAt`) SHALL NOT be carried onto the new record

#### Scenario: Registration command fails due to duplicate domain name

- **WHEN** the conditional `PutItem` fails with `ConditionalCheckFailedException` because a domain with the same name already exists in a status other than `REJECTED`
- **THEN** the handler SHALL emit `RegisterDomainFailed` to the control plane event bus with the reason "domain already exists"
- **AND THEN** NOT start any workflow

#### Scenario: Registration command fails business validation

- **WHEN** the domain command handler processes a register-domain message and a naming business rule is violated (e.g., reserved name, invalid format)
- **THEN** the handler SHALL emit `RegisterDomainFailed` to the control plane event bus with the reason
- **AND THEN** NOT persist any domain record or start any workflow

#### Scenario: Registration rejected due to missing secondary owner emails

- **WHEN** a caller invokes the register-domain API without the `secondaryOwnerEmails` field or with an empty array
- **THEN** API Gateway SHALL reject the request synchronously with `400 Bad Request`
- **AND THEN** the malformed request SHALL NOT be enqueued

#### Scenario: Environment list is snapshotted at registration time

- **WHEN** a domain is registered
- **THEN** the environment list SHALL be captured from platform configuration at that moment and stored on the domain record
- **AND THEN** subsequent changes to the platform environment configuration SHALL NOT retroactively alter the `environments` field on existing domains

#### Scenario: Registration request rejected due to structural validation

- **WHEN** a caller invokes the register-domain API with missing required fields or invalid field formats
- **THEN** API Gateway SHALL reject the request synchronously with `400 Bad Request`
- **AND THEN** the malformed request SHALL NOT be enqueued

### Requirement: Domain management events

The domain management capability SHALL emit the following events. Each event SHALL have a well-defined type, a consistent payload, and a defined emitter (control plane command handler or workflow). Events SHALL be emitted on the control plane event bus. Command outcome events (`*Succeeded` / `*Failed`) are the primary mechanism by which callers learn the result of write operations. The approval workflow SHALL be the sole emitter of approval, rejection, and approval-notification-failure events. All event payloads SHALL identify the domain by `domainName` and SHALL NOT include a `domainId` field. Events that carry domain owner information SHALL include `secondaryOwnerEmails` when present on the domain record. Events that carry `ownerAccountId` for a registered domain SHALL also carry `environmentAccounts` as the effective environment-to-account map (every environment resolved, default account applied where no override exists); the map is additive and existing consumers are unaffected.

#### Events emitted by the control plane command handler

| Event type | When emitted | Purpose | Minimum payload |
|------------|--------------|---------|-----------------|
| `RegisterDomainSucceeded` | After the register-domain command is successfully processed | Confirms domain registration was accepted | `domainName`, `ownerAccountId`, `environmentAccounts` (effective map), `ownerEmail`, `secondaryOwnerEmails`, `environments`, `status`, `createdAt` |
| `RegisterDomainFailed` | After the register-domain command fails (e.g., duplicate name, validation error) | Surfaces processing failure to callers and audit | `domainName`, `ownerAccountId`, `reason`, `failedAt` |
| `UpdateDomainSucceeded` | After the update-domain command is successfully processed | Confirms domain metadata update was applied | `domainName`, `updatedAt`, `changedFields` |
| `UpdateDomainFailed` | After the update-domain command fails (e.g., domain not found) | Surfaces processing failure to callers and audit | `domainName`, `reason`, `failedAt` |

#### Events emitted by the approval workflow

| Event type | When emitted | Purpose | Minimum payload |
|------------|--------------|---------|-----------------|
| `DomainRegistrationApproved` | After the approval workflow processes an approval decision | Triggers the provisioning workflow | `domainName`, `ownerAccountId`, `environmentAccounts` (effective map), `ownerEmail`, `secondaryOwnerEmails`, `environments`, `approvedBy`, `justification`, `approvedAt` |
| `DomainRegistrationRejected` | After the approval workflow processes a rejection decision or the approval wait times out | Triggers rejection notification; audit | `domainName`, `ownerAccountId`, `environmentAccounts` (effective map), `ownerEmail`, `secondaryOwnerEmails`, `rejectedBy`, `justification`, `rejectedAt` |
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

#### Scenario: Approval forwards the effective account map to provisioning

- **WHEN** the approval workflow emits `DomainRegistrationApproved`
- **THEN** the event payload SHALL include the effective `environmentAccounts` map resolved from the domain record
- **AND THEN** the provisioning workflow SHALL resolve each environment's target account from this map

#### Scenario: Command outcome events emitted for write operations

- **WHEN** a write command (register, update) is processed by the command handler
- **THEN** the handler SHALL emit either a `*Succeeded` or `*Failed` outcome event to the control plane bus
- **AND THEN** callers and downstream systems SHALL subscribe to these events to learn the result of their commands

#### Scenario: Notification failure event is operator-facing

- **WHEN** `DomainApprovalNotificationFailed` is emitted
- **THEN** no domain-owner-facing email notification SHALL be triggered by it (email delivery is the failed dependency)
- **AND THEN** the event SHALL be available on the control-plane bus for operator alerting and audit

### Requirement: Account-per-domain design principle

The system SHALL assume one AWS account per domain for application-side resources by default, and SHALL allow a registration to map individual environments to other accounts (account-per-domain-per-environment). The trust stack is deployed per distinct account used by any approved domain, and resource naming (`{org}-{env}-{domain}-*`) is unaffected by the mapping because names are environment-qualified. The `ownerAccountId` field on the domain record remains the default account for every environment. This principle SHALL be documented in the system architecture specification so that future changes do not silently violate it.

#### Scenario: One domain per account

- **WHEN** a domain is registered with an `ownerAccountId` and no `environmentAccounts` overrides
- **THEN** all application-side resources for all environments of that domain SHALL be provisioned in that account
- **AND THEN** the trust stack in that account SHALL grant the Konductor cross-account access to manage those resources

### Requirement: Primary and secondary owner model

The domain record SHALL store a primary owner account (`ownerAccountId`) and a primary owner email (`ownerEmail`). The domain record SHALL also store one or more secondary owner email addresses (`secondaryOwnerEmails`). At least one secondary owner email SHALL be provided at registration time. Secondary owners are co-managers of the domain who serve as backup contacts when the primary owner is unavailable or leaves the domain. Resources for each environment live in the account resolved for that environment — the primary owner's account unless an `environmentAccounts` override names another account. Secondary owner emails are captured for informational purposes at this stage — no notifications are sent to secondary owners and no authorization logic is applied to them. Both `ownerEmail` and `secondaryOwnerEmails` SHALL be mutable via the update endpoint; the `ownerAccountId` SHALL NOT be mutable after registration.

#### Scenario: Primary owner is the resource host

- **WHEN** a domain is provisioned
- **THEN** application-side resources for each environment SHALL be created in the account resolved for that environment (the primary owner's account unless overridden)
- **AND THEN** secondary owners SHALL NOT cause resources to be provisioned in additional accounts

#### Scenario: At least one secondary owner required at registration

- **WHEN** a domain is registered
- **THEN** the registration request SHALL include at least one secondary owner email address
- **AND THEN** the domain record SHALL store all provided secondary owner email addresses

#### Scenario: Owner emails are mutable

- **WHEN** a domain owner updates `ownerEmail` or `secondaryOwnerEmails` via `PUT /domains/{domainName}`
- **THEN** the corresponding field(s) SHALL be updated on the domain record in DynamoDB
- **AND THEN** the `ownerAccountId` SHALL NOT be affected

### Requirement: Update Domain

Domain owners SHALL be able to update mutable domain metadata by submitting a command that is processed asynchronously. The update command SHALL accept `description`, `ownerEmail`, `secondaryOwnerEmails`, and `costCenter` fields; all other domain attributes SHALL be immutable through this command. The domain name SHALL NOT be updatable. Status transitions SHALL NOT be performed via the update command. The `ownerAccountId` field SHALL NOT be updatable. The `environmentAccounts` field SHALL NOT be updatable. The API path SHALL be `PUT /domains/{domainName}`. The API SHALL accept the command and return immediately; the outcome SHALL be communicated via an EventBridge event.

For OAuth callers the command SHALL be authorized by an owner binding held by one of the caller's team groups on the domain, or by the `platform-admin` role. IAM callers SHALL remain authorized exactly as before.

The `UpdateDomainSucceeded` event detail SHALL carry the update command's `requestId` (the SQS message id) alongside `domainName`, `updatedAt`, `status`, and `changedFields`, so downstream consumers can key idempotent follow-up work to a single committed update.

#### Scenario: Update command accepted and enqueued

- **WHEN** a domain owner calls `PUT /domains/{domainName}` with valid fields (description, ownerEmail, secondaryOwnerEmails, and/or costCenter)
- **THEN** API Gateway SHALL validate the request structure and enqueue the command to the domains SQS queue
- **AND THEN** the `domainName` path parameter SHALL be injected into the message body by the API Gateway VTL mapping template
- **AND THEN** the API SHALL return `202 Accepted` with `{"requestId":"<SQS-MessageId>","message":"Domain update request accepted"}`

#### Scenario: OAuth update requires ownership

- **WHEN** an OAuth caller without the `platform-admin` role submits an update-domain command for a domain their team groups do not own
- **THEN** the command SHALL be denied with `AccessDenied`
- **AND THEN** no update SHALL be applied and no event SHALL be emitted

#### Scenario: Update secondary owner emails

- **WHEN** a domain owner calls `PUT /domains/{domainName}` with a `secondaryOwnerEmails` field containing one or more valid email addresses
- **THEN** the command handler SHALL replace the entire `secondaryOwnerEmails` set on the domain record in DynamoDB
- **AND THEN** emit `UpdateDomainSucceeded` with `secondaryOwnerEmails` in the `changedFields` list

#### Scenario: Update primary owner email

- **WHEN** a domain owner calls `PUT /domains/{domainName}` with an `ownerEmail` field containing a valid email address
- **THEN** the command handler SHALL update the `ownerEmail` field on the domain record in DynamoDB
- **AND THEN** emit `UpdateDomainSucceeded` with `ownerEmail` in the `changedFields` list
- **AND THEN** subsequent notification emails SHALL be sent to the new owner email address

#### Scenario: Update cost center

- **WHEN** a domain owner calls `PUT /domains/{domainName}` with a valid non-empty `costCenter`
- **THEN** the command handler SHALL update the `costCenter` field on the domain record in DynamoDB and recompute the record's stored effective `tags` map to carry the new `cost-center` value
- **AND THEN** emit `UpdateDomainSucceeded` with `costCenter` in the `changedFields` list and the new `costCenter` value and command `requestId` in the event detail

#### Scenario: Invalid cost center update fails

- **WHEN** an update-domain command carries a `costCenter` that fails the governed-tag validation (over 256 characters or containing control characters)
- **THEN** no update SHALL be applied to the domain record
- **AND THEN** the handler SHALL emit `UpdateDomainFailed` with the validation reason

#### Scenario: Update command processed and succeeded event emitted

- **WHEN** the domain command handler processes an update-domain message
- **THEN** the handler SHALL apply the changes to the domain record in DynamoDB keyed by `domainName`
- **AND THEN** emit `UpdateDomainSucceeded` to the control plane event bus

#### Scenario: Update command fails

- **WHEN** the domain command handler processes an update-domain message and the update cannot be applied (e.g., domain not found, domain not in ACTIVE status)
- **THEN** the handler SHALL emit `UpdateDomainFailed` with the reason

### Requirement: Provisioning retry for failed domains

A platform admin SHALL be able to retry provisioning for a domain in `PROVISIONING_FAILED` status via `POST /domains/{domainName}/state` with action `retry-provisioning` on the OAuth control-plane surface. The action SHALL conditionally transition the domain from `PROVISIONING_FAILED` to `APPROVED` (clearing `errorDetail`) and SHALL emit a `DomainProvisioningRetryRequested` lifecycle event on the control plane bus carrying the registration fields recorded on the domain record (`domainName`, `ownerAccountId`, the effective `environmentAccounts` map, `ownerEmail`, `secondaryOwnerEmails`, `environments`, plus the retrying principal and timestamp). The provisioning workflow SHALL start on that event exactly as it does for `DomainRegistrationApproved`. The retry SHALL fail with `409 Conflict` when the domain is not in `PROVISIONING_FAILED`; it SHALL NOT create, modify, or re-create role bindings (they already exist from the original approval). Team callers SHALL be denied with `403`. The IAM surface SHALL NOT expose retry in this capability version.

#### Scenario: Admin retries a failed domain

- **WHEN** a platform admin submits `retry-provisioning` for a domain in `PROVISIONING_FAILED`
- **THEN** the domain SHALL transition to `APPROVED` with `errorDetail` cleared
- **AND THEN** `DomainProvisioningRetryRequested` SHALL be emitted with the domain's recorded registration fields
- **AND THEN** the provisioning workflow SHALL start for the domain

#### Scenario: Retry rejected when not failed

- **WHEN** a platform admin submits `retry-provisioning` for a domain in any status other than `PROVISIONING_FAILED`
- **THEN** the API SHALL return `409 Conflict` and no lifecycle event SHALL be emitted

#### Scenario: Team caller cannot retry

- **WHEN** a caller without the `platform-admin` role submits `retry-provisioning`
- **THEN** the API SHALL return `403 Forbidden` and the domain SHALL remain unchanged
