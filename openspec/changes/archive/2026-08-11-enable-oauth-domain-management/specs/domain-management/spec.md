# Delta: domain-management

## MODIFIED Requirements

### Requirement: Register Domain

Domain management is an administrative function of the control plane. Any authenticated OAuth caller who belongs to at least one team group SHALL be able to request registration of a new domain by submitting a command that is processed asynchronously; IAM callers SHALL remain able to register domains exactly as before. The API SHALL accept the command and return immediately with a request ID (SQS message ID) and a descriptive message. Each request SHALL go through an approval process; only after approval SHALL the provisioning workflow be triggered. All domain details SHALL be stored in the single domains DynamoDB table. The domain name SHALL be used as the DynamoDB partition key. Uniqueness SHALL be enforced atomically via a conditional `PutItem` with `attribute_not_exists(domainName) OR #status = :REJECTED` — the system SHALL NOT perform a pre-write query to check for duplicates. A `REJECTED` record SHALL NOT block its domain name: a new registration for that name SHALL atomically replace the rejected record with a fresh registration (new `requestId`, status `PENDING_APPROVAL`, fresh approval cycle); rejection history remains available through the `DomainRegistrationRejected` events on the control plane bus.

For OAuth callers the registration request SHALL carry an `owningTeam` naming exactly one of the caller's Cognito team groups, which SHALL be recorded on the domain record and used to write the owner binding at approval (see domain-ownership). IAM callers MAY omit `owningTeam`; when omitted, no owner binding is created at approval and ownership administration falls to the platform team.

The set of environments a domain is provisioned into SHALL be determined by the **platform team**, not the requester. The Konductor platform SHALL hold a single configured environment list (default `["dev", "staging", "prod"]`), and every approved domain SHALL be provisioned into the full list. The registration API SHALL NOT accept an `environments` field. All environments of a given domain SHALL be provisioned into the same `ownerAccountId`.

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
- **THEN** the handler SHALL validate business rules (naming, reserved names)
- **AND THEN** read the platform-configured environment list
- **AND THEN** persist the domain in DynamoDB using a conditional `PutItem` with `domainName` as the partition key and a condition expression of `attribute_not_exists(domainName) OR #status = :REJECTED` and status `PENDING_APPROVAL` and that environment list stamped onto the `environments` field of the domain record, and `secondaryOwnerEmails` stored as a String Set
- **AND THEN** start the approval Step Functions workflow
- **AND THEN** emit `RegisterDomainSucceeded` to the control plane event bus (including the stamped `environments` list and `secondaryOwnerEmails`)

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

Users SHALL be able to list the domains they have permission to access, with filters and pagination that scale to large organizations. Visibility SHALL be scoped by caller: platform roles (`platform-admin`, `platform-auditor`) SHALL receive the full list; OAuth callers without a platform role SHALL receive only domains owned by their team groups (owner bindings) with the full record; IAM callers SHALL remain unscoped. The response SHALL identify each domain by `domainName` and SHALL NOT include a `domainId` field. Each item SHALL include `secondaryOwnerEmails` when present on the domain record. The `taskToken` field SHALL NOT be included in list responses for any caller.

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

### Requirement: Update Domain

Domain owners SHALL be able to update mutable domain metadata by submitting a command that is processed asynchronously. The update command SHALL accept `description`, `tags`, `ownerEmail`, and `secondaryOwnerEmails` fields; all other domain attributes SHALL be immutable through this command. The domain name SHALL NOT be updatable. Status transitions SHALL NOT be performed via the update command. The `ownerAccountId` field SHALL NOT be updatable. The API path SHALL be `PUT /domains/{domainName}`. The API SHALL accept the command and return immediately; the outcome SHALL be communicated via an EventBridge event.

For OAuth callers the command SHALL be authorized by an owner binding held by one of the caller's team groups on the domain, or by the `platform-admin` role. IAM callers SHALL remain authorized exactly as before.

#### Scenario: Update command accepted and enqueued

- **WHEN** a domain owner calls `PUT /domains/{domainName}` with valid fields (description, tags, ownerEmail, and/or secondaryOwnerEmails)
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

## ADDED Requirements

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
