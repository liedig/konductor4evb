# Capability: Domain Management

## Purpose

How event domains are registered, approved, and managed through their lifecycle: the domains table as the single system of record, the register/list/describe/update/decide control-plane operations, the domain status model, lifecycle events emitted on the control-plane bus, and the ownership and account-per-domain design principles every other capability builds on.

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

### Requirement: List Domains

Users SHALL be able to list the domains they have permission to access, with filters and pagination that scale to large organizations. Visibility SHALL be scoped by caller: platform roles (`platform-admin`, `platform-auditor`) SHALL receive the full list; OAuth callers without a platform role SHALL receive only domains owned by their team groups (owner bindings) with the full record, plus their team groups' own registration requests still in `PENDING_APPROVAL` or `REJECTED` (matched by `owningTeam`, since bindings do not exist before approval); IAM callers SHALL remain unscoped. The response SHALL identify each domain by `domainName` and SHALL NOT include a `domainId` field. Each item SHALL include `secondaryOwnerEmails` when present on the domain record. The `taskToken` field SHALL NOT be included in list responses for any caller.

The API SHALL additionally provide a names-only directory projection (e.g. `GET /domains?view=directory`) returning only `domainName`, `description`, and `status` per domain, available to any authenticated caller, to back pickers such as the entitlement-grant form.

#### Scenario: List domains with default filters

- **WHEN** a user calls the list-domains API with no filters
- **THEN** the control plane SHALL return all domains the caller is authorized to see
- **AND THEN** include basic metadata such as name, description, owner, ownerEmail, secondaryOwnerEmails, creation date, and status for each domain
- **AND THEN** return results in a paginated form that can scale to thousands of domains
- **AND THEN** the `taskToken` field SHALL NOT be included in the response

#### Scenario: Platform caller lists all domains

- **WHEN** a platform-admin or platform-auditor calls the list-domains API
- **THEN** the control plane SHALL return all domains with full records
- **AND THEN** return results in a paginated form that can scale to thousands of domains
- **AND THEN** the `taskToken` field SHALL NOT be included in the response

#### Scenario: Team caller lists owned domains only

- **WHEN** an OAuth caller without a platform role calls the list-domains API
- **THEN** the control plane SHALL return only domains whose owner bindings match the caller's team groups
- **AND THEN** include basic metadata such as name, description, owner, ownerEmail, secondaryOwnerEmails, creation date, and status for each domain
- **AND THEN** return results in a paginated form

#### Scenario: Directory projection is names-only and open to authenticated callers

- **WHEN** any authenticated caller requests the directory projection
- **THEN** the control plane SHALL return every domain's `domainName`, `description`, and `status`
- **AND THEN** the projection SHALL NOT include owner account IDs, owner emails, secondary owner emails, resource identifiers, or task tokens

#### Scenario: List domains with filters

- **WHEN** a user calls the list-domains API with filters such as owner account, tags, or status
- **THEN** the control plane SHALL apply those filters server-side within the caller's authorized visibility
- **AND THEN** return only matching domains along with pagination tokens where needed

#### Scenario: List domains filtered by PENDING_APPROVAL status

- **WHEN** a user calls the list-domains API with `status=PENDING_APPROVAL`
- **THEN** the control plane SHALL return domains awaiting approval within the caller's authorized visibility
- **AND THEN** this SHALL serve as the replacement for the former list-pending-activations endpoint

### Requirement: Describe Domain

Users SHALL be able to retrieve detailed information about a specific domain by its domain name, including configuration, resources, and approval metadata. The API path SHALL be `GET /domains/{domainName}`. OAuth callers without a platform role SHALL be authorized only when one of their team groups holds the domain's owner binding. The `taskToken` field SHALL NOT be returned to domain owner callers. Platform team callers MAY access the `taskToken` when the domain status is `PENDING_APPROVAL`. Resource identifiers SHALL be returned as a map keyed by environment, where each entry contains the per-environment resource ARNs. The response SHALL include `secondaryOwnerEmails` when present on the domain record.

#### Scenario: Describe existing domain

- **WHEN** a user calls `GET /domains/{domainName}` with a valid domain name they are authorized to access
- **THEN** the control plane SHALL perform a `GetItem` on the domains table using `domainName` as the partition key
- **AND THEN** return domain configuration (name, description, owner, ownerEmail, secondaryOwnerEmails, tags, status, environments)
- **AND THEN** return resource identifiers as a map keyed by environment when the domain is `ACTIVE`; each entry SHALL contain `platformEventBusArn`, `appEventBusArn`, `schemaRegistryArn`, `subscriptionDlqArn`, `integrationDlqArn`, and `dashboardArn`
- **AND THEN** return domain-level identifiers (namespace parameter path, RAM share ARN) at the top level, not nested under any environment
- **AND THEN** return approval metadata (decidedBy, justification, approvedAt/rejectedAt) when available
- **AND THEN** the `taskToken` field SHALL NOT be included in the response for domain owner callers

#### Scenario: Describe unknown or unauthorized domain

- **WHEN** a user calls `GET /domains/{domainName}` with a domain name that does not exist or that they are not authorized to access
- **THEN** the control plane SHALL not leak domain existence details
- **AND THEN** return an error that represents either not-found or access-denied according to project security conventions

### Requirement: Decide Registration

Platform team members SHALL be able to approve or reject a domain registration request only through the OAuth control-plane surface. The synchronous endpoint SHALL require an authenticated trusted principal with the `platform-admin` global role. It SHALL accept the Step Functions task token, decision, and justification, but SHALL derive `decidedBy` from the trusted OAuth principal rather than a caller-asserted request field. Possession of a task token SHALL NOT authorize the caller. The Lambda SHALL call `SendTaskSuccess` or `SendTaskFailure` to resume the approval workflow and SHALL NOT read from DynamoDB, emit events, or update domain status.

#### Scenario: Approval decision submitted successfully

- **WHEN** an OAuth-authenticated `platform-admin` calls `POST /domains/registrations/decide` with a valid task token, decision of `ACCEPT`, and optional justification
- **THEN** the decide Lambda SHALL call `sfn:SendTaskSuccess` with the task token and an output payload containing `approved: true`, trusted principal-derived `decidedBy`, and `justification`
- **AND THEN** the Lambda SHALL return `200 OK` with `{"message":"Decision recorded"}`

#### Scenario: Rejection decision submitted successfully

- **WHEN** an OAuth-authenticated `platform-admin` calls `POST /domains/registrations/decide` with a valid task token, decision of `REJECT`, and required justification
- **THEN** the decide Lambda SHALL call `sfn:SendTaskFailure` with the task token, error code `RegistrationRejected`, and cause containing trusted principal-derived `decidedBy` and `justification`
- **AND THEN** the Lambda SHALL return `200 OK` with `{"message":"Decision recorded"}`

#### Scenario: Bearer task token is not caller authorization

- **WHEN** a caller presents a valid task token without a valid OAuth principal carrying `platform-admin`
- **THEN** API Gateway SHALL reject the request before invoking the decide Lambda
- **AND THEN** no Step Functions callback SHALL be made

#### Scenario: Caller asserts a decider identity

- **WHEN** a decision request contains a caller-supplied `decidedBy` value
- **THEN** that value SHALL NOT be used as the authoritative decider identity
- **AND THEN** the workflow callback and audit record SHALL identify the validated OAuth principal

#### Scenario: Decision with missing or invalid fields

- **WHEN** an authorized caller submits a decision with a missing task token or decision, or an invalid decision value
- **THEN** the request SHALL be rejected with `400 Bad Request` and a descriptive validation error
- **AND THEN** no Step Functions callback SHALL be made

#### Scenario: Decision with invalid or expired task token

- **WHEN** an authorized caller submits a decision with a task token that is expired, already used, or invalid
- **THEN** the `sfn:SendTaskSuccess` or `sfn:SendTaskFailure` call SHALL fail
- **AND THEN** the Lambda SHALL return a safe error response indicating that the callback failed

#### Scenario: Justification required on rejection

- **WHEN** an authorized caller submits a decision of `REJECT` without a `justification` field
- **THEN** the Lambda SHALL return `400 Bad Request` indicating that justification is required for rejections
- **AND THEN** no Step Functions callback SHALL be made

### Requirement: Pending registrations endpoint for approvers

The OAuth control plane SHALL expose `GET /domains/registrations/pending` restricted to the `platform-admin` global role, returning every domain in `PENDING_APPROVAL` status with the fields an approver needs to decide: domain name, description, owner account ID, owner email, secondary owner emails, owning team, requested-at timestamp, and the Step Functions `taskToken`. This endpoint SHALL be the only read path that projects `taskToken` for non-workflow consumers.

#### Scenario: Platform admin lists pending registrations

- **WHEN** an OAuth-authenticated `platform-admin` calls `GET /domains/registrations/pending`
- **THEN** the control plane SHALL return all `PENDING_APPROVAL` domains including their `taskToken` values
- **AND THEN** the caller MAY pass a returned `taskToken` to the decide endpoint

#### Scenario: Non-admin callers are denied

- **WHEN** a caller without the `platform-admin` role calls `GET /domains/registrations/pending`
- **THEN** the control plane SHALL return `403` access denied
- **AND THEN** no task token SHALL be disclosed

#### Scenario: Empty inbox

- **WHEN** a platform-admin calls the endpoint and no registrations are pending
- **THEN** the control plane SHALL return `200 OK` with an empty collection

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
| `INACTIVE` | Domain disabled by the platform team: publishing and new subscription activations are rejected; existing subscriptions, entitlements, and provisioned infrastructure are retained | Platform team |

#### Scenario: Valid status transitions

- **WHEN** a domain status transition occurs
- **THEN** only the following transitions SHALL be permitted:
  - `PENDING_APPROVAL` → `APPROVED` (approval workflow, approved path)
  - `PENDING_APPROVAL` → `REJECTED` (approval workflow, rejected or timeout path)
  - `PENDING_APPROVAL` → `APPROVAL_NOTIFICATION_FAILED` (approval workflow, notification-failure path)
  - `APPROVED` → `PROVISIONING` (provisioning workflow, first step)
  - `PROVISIONING` → `ACTIVE` (provisioning workflow, success)
  - `PROVISIONING` → `PROVISIONING_FAILED` (provisioning workflow, failure)
  - `ACTIVE` → `INACTIVE` (platform-team disable)
  - `INACTIVE` → `ACTIVE` (platform-team enable)

#### Scenario: Invalid status transition rejected

- **WHEN** any component attempts a status transition not listed in the valid transitions
- **THEN** the transition SHALL be rejected
- **AND THEN** the domain record SHALL remain in its current status

#### Scenario: Notification failure is not a rejection

- **WHEN** the approval workflow cannot deliver the approval request notification
- **THEN** the domain SHALL transition to `APPROVAL_NOTIFICATION_FAILED`, not `REJECTED`
- **AND THEN** the record SHALL indicate operator intervention is required (recovery is operator-driven; no automated transition out of this status is defined)

### Requirement: Platform-managed domain enable and disable

The control plane SHALL let the platform team disable and re-enable an `ACTIVE`/`INACTIVE` domain via `POST /domains/{domainName}/state` restricted to the `platform-admin` global role. While a domain is `INACTIVE`, the control plane SHALL reject new publish commands and new subscription activations targeting the domain, SHALL reject entitlement and subscription administration for the domain, and SHALL retain all existing subscriptions, entitlements, and provisioned infrastructure unchanged.

#### Scenario: Platform admin disables a domain

- **WHEN** an OAuth-authenticated `platform-admin` calls `POST /domains/{domainName}/state` with action `disable` on an `ACTIVE` domain
- **THEN** the control plane SHALL transition the domain to `INACTIVE`
- **AND THEN** subsequent publish and subscribe-activation attempts for the domain SHALL be rejected with a descriptive error

#### Scenario: Platform admin re-enables a domain

- **WHEN** an OAuth-authenticated `platform-admin` calls `POST /domains/{domainName}/state` with action `enable` on an `INACTIVE` domain
- **THEN** the control plane SHALL transition the domain to `ACTIVE`
- **AND THEN** publishing and subscription activation SHALL resume

#### Scenario: Non-admin callers cannot change domain state

- **WHEN** a caller without the `platform-admin` role calls `POST /domains/{domainName}/state`
- **THEN** the control plane SHALL return `403` access denied
- **AND THEN** the domain status SHALL remain unchanged

#### Scenario: Disable retains existing estate

- **WHEN** a domain with active subscriptions and provisioned infrastructure is disabled
- **THEN** those subscriptions and resources SHALL remain in place and unchanged
- **AND THEN** only new publish and activation activity SHALL be rejected

### Requirement: Domain name as primary identifier

The domain name SHALL be the sole identifier for a domain throughout the system. Domain names SHALL be globally unique within the Konductor installation. This uniqueness is an intentional governance feature that forces domain owners to make explicit bounded-context decisions, aligning with DDD and EDA principles. The system SHALL NOT generate or use a surrogate identifier (e.g., UUID) for domains. API paths, event payloads, DynamoDB keys, and Step Functions inputs SHALL all use the domain name as the domain identifier.

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

### Requirement: Registration preflight resource-collision check

Before persisting a registration, the command handler SHALL check for colliding platform-owned resource names for the requested domain across the platform-configured environment list: the per-environment event bus name (`{org}-{env}-{domain}-event-bus`) and the domain namespace SSM parameter (`/namespace/{domain}`). When any collision exists, the handler SHALL persist the record with status `REJECTED`, `decidedBy: system`, and a justification naming the colliding resources; it SHALL NOT start the approval workflow; it SHALL emit `RegisterDomainFailed` with the collision detail. Because the rejected record does not block its name, the requester SHALL be able to re-request the domain once the collision is cleared. When the collision check itself errors (e.g. throttling), the handler SHALL fail the command and leave the message to the queue's retry/DLQ semantics rather than guessing.

#### Scenario: Collision produces a deterministic system rejection

- **WHEN** a registration command is processed for a domain whose per-environment event bus or namespace parameter already exists
- **THEN** the record SHALL be persisted as `REJECTED` with `decidedBy: system` and a justification naming the colliding resources
- **AND THEN** no approval workflow SHALL start and no bindings SHALL be created

#### Scenario: Requester sees and re-requests after cleanup

- **WHEN** the colliding resources are removed and the requester re-submits the same domain name
- **THEN** the conditional write SHALL replace the system-rejected record with a fresh `PENDING_APPROVAL` registration
- **AND THEN** the approval workflow SHALL start normally

#### Scenario: No collision proceeds unchanged

- **WHEN** a registration command is processed and no colliding resources exist
- **THEN** the domain SHALL be persisted as `PENDING_APPROVAL` and the approval workflow SHALL start, exactly as before

### Requirement: Domain resource tagging

Every domain SHALL carry a standard tag set, applied by the platform: `konductor:managed-by` (`konductor`), `konductor:domain` (the domain name), `konductor:environment` (per environment on per-environment resources), and `konductor:namespace` (the `{org}-{domain}` namespace identity). A registration MAY additionally supply a single governed custom tag, `costCenter`, for cost allocation; the API SHALL reject any other custom tag keys. The domain record SHALL store the effective tag set (standard set plus `cost-center` when supplied), and describe and list responses SHALL render it. `cost-center` SHALL be settable at registration and SHALL be mutable afterwards via the update command with change-only semantics: an update MAY set a `costCenter` where none exists and MAY replace an existing value with another valid value, but SHALL NOT clear an existing value (the API schema makes clearing inexpressible by requiring a non-empty value). A registration whose `costCenter` fails the governed-tag validation SHALL fail before any record is written.

#### Scenario: Registration with a cost center

- **WHEN** a caller submits a registration with `costCenter: "cc-1234"`
- **THEN** the record SHALL persist `costCenter`
- **AND THEN** describe SHALL return the standard tag set plus `cost-center=cc-1234` as the effective tags

#### Scenario: Arbitrary tag keys rejected

- **WHEN** a caller submits a registration with a `tags` map or any custom key other than `costCenter`
- **THEN** the API SHALL return `400 ValidationException`

#### Scenario: No cost center

- **WHEN** a registration omits `costCenter`
- **THEN** the record SHALL carry the standard tag set only and registration SHALL proceed normally

#### Scenario: Cost center is replaceable via update

- **WHEN** an owner submits a domain update carrying a valid `costCenter` different from the stored value
- **THEN** the domain record SHALL store the new `costCenter` and a recomputed effective `tags` map
- **AND THEN** describe SHALL render the new `cost-center` tag value

#### Scenario: Cost center cannot be cleared

- **WHEN** an owner submits a domain update with `costCenter` set to an empty string
- **THEN** the API SHALL return `400 ValidationException`
- **AND THEN** the stored value SHALL remain unchanged

#### Scenario: Invalid cost center at registration is rejected

- **WHEN** a registration carries a `costCenter` that fails the governed-tag validation (empty, over 256 characters, or containing control characters)
- **THEN** the command SHALL fail validation and emit `RegisterDomainFailed` with the validation reason
- **AND THEN** no domain record SHALL be persisted, the domain count SHALL NOT be charged, and no workflow SHALL be started

### Requirement: Domain count is capped

Domain registration SHALL enforce a ceiling of 1,000 domains per platform deployment. Creating a new domain beyond the ceiling SHALL fail deterministically: no domain row is created, no approval workflow starts, and the failure SHALL be reported through the established `RegisterDomainFailed` outcome event carrying a limit-exceeded reason, with the command message acknowledged (no retry, since retrying cannot change the outcome). The ceiling check SHALL be atomic with domain creation so concurrent registrations cannot exceed it. The ceiling SHALL apply to every new domain row, including system-rejected rows recorded by the registration collision preflight. Re-registration of a rejected domain and updates to existing domains SHALL NOT be affected by the ceiling.

#### Scenario: Registration beyond the cap is rejected

- **WHEN** 1,000 domains exist
- **AND WHEN** a caller registers a new domain
- **THEN** the registration SHALL fail with a limit-exceeded reason
- **AND THEN** no domain row SHALL be created, no approval workflow SHALL start, a `RegisterDomainFailed` event SHALL carry the limit reason, and the command message SHALL be acknowledged without retry

#### Scenario: Concurrent registrations cannot race past the cap

- **WHEN** 999 domains exist
- **AND WHEN** two registrations for different new domains commit concurrently
- **THEN** exactly one SHALL succeed
- **AND THEN** the other SHALL fail with the limit-exceeded reason through the same deterministic path

#### Scenario: Re-registering a rejected domain is not capped

- **WHEN** 1,000 domains exist and one of them is rejected
- **AND WHEN** a caller re-registers the rejected domain
- **THEN** the ceiling SHALL NOT block the re-registration

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

### Requirement: Latest system-event marker discovery

The domain list and detail APIs SHALL expose optional `systemEventsLastEventAt` and `systemEventsLastEventType` from the domain record so the console can compute its unread indicator from the list endpoint alone. The marker SHALL appear only when recorded and SHALL remain absent for domains without any delivered event. The topic ARN SHALL remain describe-only.

#### Scenario: List carries the marker but not the ARN
- **WHEN** an authorized caller lists domains and one has a recorded marker
- **THEN** the summary SHALL contain the marker timestamp and event type
- **AND** it SHALL still omit `systemEventsTopicArn`

#### Scenario: No events delivered yet
- **WHEN** a domain has a topic but no recorded marker
- **THEN** both APIs SHALL omit the marker fields
- **AND** the console SHALL treat the domain as seen

### Requirement: Domain system-events topic discovery

The OAuth and IAM domain-detail APIs SHALL expose optional top-level `systemEventsTopicArn` from the domain record. The field SHALL appear only after topic configuration is recorded and SHALL not be synthesized from a naming convention. It SHALL remain outside per-environment resource maps and preserve existing domain visibility rules.

#### Scenario: Owner discovers a configured topic
- **WHEN** an authorized domain owner describes a domain whose topic has been provisioned or backfilled
- **THEN** the response SHALL contain the recorded topic ARN once at domain scope
- **AND** that ARN SHALL be identical regardless of API stage

#### Scenario: Topic has not been configured
- **WHEN** an authorized caller describes a domain with no recorded topic ARN
- **THEN** the response SHALL omit `systemEventsTopicArn`
- **AND** the existing domain details SHALL remain readable

#### Scenario: Both authentication surfaces expose the field
- **WHEN** authorized OAuth and IAM callers describe the same domain
- **THEN** both response contracts SHALL expose the same recorded topic ARN
- **AND** the new field SHALL not introduce a new authorization path

#### Scenario: Topic discovery does not leak through directory lookup
- **WHEN** a caller uses the names-only domain directory or cannot access a domain's detail
- **THEN** the system SHALL not reveal the topic ARN through that response
- **AND** unauthorized detail requests SHALL preserve the existing not-found or access-denied behavior

### Requirement: Owner email change reconciles the topic subscription

A committed `ownerEmail` change SHALL trigger reconciliation of the domain's system-events topic default email subscription: the previous address is unsubscribed and the new address subscribed through the platform's topic-management path. The reconciliation SHALL NOT be accepted as a mutable domain field or performed by domain callers directly, and its failure SHALL NOT reverse the committed update.

#### Scenario: Owner change re-points the default subscription
- **WHEN** an authorized caller commits an owner-email change for a domain with a recorded topic
- **THEN** the topic's default email subscription SHALL be moved from the old address to the new one
- **AND** the success event SHALL carry both `ownerEmail` and `previousOwnerEmail` so the reconciliation consumer never guesses the replaced address
- **AND** the new address SHALL receive a fresh SNS confirmation request

#### Scenario: Reconciliation failure does not roll back the update
- **WHEN** subscription reconciliation fails after the owner change committed
- **THEN** the domain record SHALL retain the new owner email
- **AND** the failure SHALL remain observable for operator retry

#### Scenario: Domain without a topic skips reconciliation
- **WHEN** the owner email changes for a domain with no recorded topic ARN
- **THEN** reconciliation SHALL be skipped without failing the update

### Requirement: Topic configuration remains platform-owned

Domain registration and update APIs SHALL not accept system-event topic destinations, topic policies, or endpoint subscriptions as mutable domain fields. Discovering an ARN SHALL not itself authorize SNS access; domain accounts SHALL subscribe through AWS using the topic policy and their own permissions.

#### Scenario: Caller supplies a replacement topic
- **WHEN** a domain update attempts to replace `systemEventsTopicArn`
- **THEN** it SHALL not change the stored topic or its policy
- **AND** public update schemas SHALL not declare the field writable
