## MODIFIED Requirements

### Requirement: Register Domain

Domain management is an administrative function of the control plane. Domain owners SHALL be able to request registration of a new domain by submitting a command that is processed asynchronously. The API SHALL accept the command and return immediately with a request ID (SQS message ID) and a descriptive message. Each request SHALL go through an approval process; only after approval SHALL the provisioning workflow be triggered. All domain details SHALL be stored in the single domains DynamoDB table. The domain name SHALL be used as the DynamoDB partition key. Uniqueness SHALL be enforced atomically via a conditional `PutItem` with `attribute_not_exists(domainName)` — the system SHALL NOT perform a pre-write query to check for duplicates.

The set of environments a domain is provisioned into SHALL be determined by the **platform team**, not the requester. The Nexus platform SHALL hold a single configured environment list (default `["dev", "staging", "prod"]`), and every approved domain SHALL be provisioned into the full list. The registration API SHALL NOT accept an `environments` field. All environments of a given domain SHALL be provisioned into the same `ownerAccountId`.

The registration request SHALL include at least one secondary owner email address. Secondary owners are co-managers of the domain who serve as backup contacts when the primary owner is unavailable. At this stage, secondary owner emails are captured for informational purposes only — no notifications are sent to secondary owners.

#### Scenario: Registration command accepted and enqueued

- **WHEN** an authorized domain owner calls the register-domain API with a valid domain name, owner account ID, owner email, at least one secondary owner email, and optional description/tags
- **THEN** API Gateway SHALL validate the request structure (including `secondaryOwnerEmails` with `minItems: 1`) and enqueue the command to the domains SQS queue
- **AND THEN** the API SHALL return `202 Accepted` with `{"requestId":"<SQS-MessageId>","message":"Domain registration request accepted"}`

#### Scenario: Registration command processed and succeeded event emitted

- **WHEN** the domain command handler processes a register-domain message from the queue
- **THEN** the handler SHALL validate business rules (naming, reserved names)
- **AND THEN** read the platform-configured environment list
- **AND THEN** persist the domain in DynamoDB using a conditional `PutItem` with `domainName` as the partition key and a condition expression of `attribute_not_exists(domainName)` and status `PENDING_APPROVAL` and that environment list stamped onto the `environments` field of the domain record, and `secondaryOwnerEmails` stored as a String Set
- **AND THEN** start the approval Step Functions workflow
- **AND THEN** emit `RegisterDomainSucceeded` to the control plane event bus (including the stamped `environments` list and `secondaryOwnerEmails`)

#### Scenario: Registration command fails due to duplicate domain name

- **WHEN** the conditional `PutItem` fails with `ConditionalCheckFailedException` because a domain with the same name already exists
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

### Requirement: Describe Domain

Users SHALL be able to retrieve detailed information about a specific domain by its domain name, including configuration, resources, and approval metadata. The API path SHALL be `GET /domains/{domainName}`. The `taskToken` field SHALL NOT be returned to domain owner callers. Platform team callers MAY access the `taskToken` when the domain status is `PENDING_APPROVAL`. Resource identifiers SHALL be returned as a map keyed by environment, where each entry contains the per-environment resource ARNs. The response SHALL include `secondaryOwnerEmails` when present on the domain record.

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

### Requirement: List Domains

Users SHALL be able to list the domains they have permission to access, with filters and pagination that scale to large organizations. The response SHALL identify each domain by `domainName` and SHALL NOT include a `domainId` field. Each item SHALL include `secondaryOwnerEmails` when present on the domain record.

#### Scenario: List domains with default filters

- **WHEN** a user calls the list-domains API with no filters
- **THEN** the control plane SHALL return all domains the caller is authorized to see
- **AND THEN** include basic metadata such as name, description, owner, ownerEmail, secondaryOwnerEmails, creation date, and status for each domain
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

### Requirement: Update Domain

Domain owners SHALL be able to update mutable domain metadata by submitting a command that is processed asynchronously. The update command SHALL accept `description`, `tags`, `ownerEmail`, and `secondaryOwnerEmails` fields. The domain name SHALL NOT be updatable. Status transitions SHALL NOT be performed via the update command. The `ownerAccountId` field SHALL NOT be updatable. The API path SHALL be `PUT /domains/{domainName}`. The API SHALL accept the command and return immediately; the outcome SHALL be communicated via an EventBridge event.

#### Scenario: Update command accepted and enqueued

- **WHEN** a domain owner calls `PUT /domains/{domainName}` with valid fields (description, tags, ownerEmail, and/or secondaryOwnerEmails)
- **THEN** API Gateway SHALL validate the request structure and enqueue the command to the domains SQS queue
- **AND THEN** the `domainName` path parameter SHALL be injected into the message body by the API Gateway VTL mapping template
- **AND THEN** the API SHALL return `202 Accepted` with `{"requestId":"<SQS-MessageId>","message":"Domain update request accepted"}`

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

### Requirement: Domain management events

The domain management capability SHALL emit the following events. Each event SHALL have a well-defined type, a consistent payload, and a defined emitter (control plane command handler or workflow). Events SHALL be emitted on the control plane event bus. Command outcome events (`*Succeeded` / `*Failed`) are the primary mechanism by which callers learn the result of write operations. The approval workflow SHALL be the sole emitter of approval and rejection events. All event payloads SHALL identify the domain by `domainName` and SHALL NOT include a `domainId` field. Events that carry domain owner information SHALL include `secondaryOwnerEmails` when present on the domain record.

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
| `DomainRegistrationRejected` | After the approval workflow processes a rejection decision | Triggers rejection notification; audit | `domainName`, `ownerAccountId`, `ownerEmail`, `secondaryOwnerEmails`, `rejectedBy`, `justification`, `rejectedAt` |

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

### Requirement: Primary and secondary owner model

The domain record SHALL store a primary owner account (`ownerAccountId`) and a primary owner email (`ownerEmail`). The domain record SHALL also store one or more secondary owner email addresses (`secondaryOwnerEmails`). At least one secondary owner email SHALL be provided at registration time. Secondary owners are co-managers of the domain who serve as backup contacts when the primary owner is unavailable or leaves the domain. Resources always live in the primary owner's account. Secondary owner emails are captured for informational purposes at this stage — no notifications are sent to secondary owners and no authorization logic is applied to them. Both `ownerEmail` and `secondaryOwnerEmails` SHALL be mutable via the update endpoint; the `ownerAccountId` SHALL NOT be mutable after registration.

#### Scenario: Primary owner is the resource host

- **WHEN** a domain is provisioned
- **THEN** all application-side resources SHALL be created in the primary owner's account
- **AND THEN** secondary owners SHALL NOT cause resources to be provisioned in additional accounts

#### Scenario: At least one secondary owner required at registration

- **WHEN** a domain is registered
- **THEN** the registration request SHALL include at least one secondary owner email address
- **AND THEN** the domain record SHALL store all provided secondary owner email addresses

#### Scenario: Owner emails are mutable

- **WHEN** a domain owner updates `ownerEmail` or `secondaryOwnerEmails` via `PUT /domains/{domainName}`
- **THEN** the corresponding field(s) SHALL be updated on the domain record in DynamoDB
- **AND THEN** the `ownerAccountId` SHALL NOT be affected
