## MODIFIED Requirements

### Requirement: Register Domain

Domain management is an administrative function of the control plane. Domain owners SHALL be able to request registration of a new domain by submitting a command that is processed asynchronously. The API SHALL accept the command and return immediately with a request ID (SQS message ID) and a descriptive message. Each request SHALL go through an approval process; only after approval SHALL the provisioning workflow be triggered. All domain details SHALL be stored in the single domains DynamoDB table.

The set of environments a domain is provisioned into SHALL be determined by the **platform team**, not the requester. The Nexus platform SHALL hold a single configured environment list (default `["dev", "staging", "prod"]`), and every approved domain SHALL be provisioned into the full list. The registration API SHALL NOT accept an `environments` field. All environments of a given domain SHALL be provisioned into the same `ownerAccountId`.

#### Scenario: Registration command accepted and enqueued

- **WHEN** an authorized domain owner calls the register-domain API with a valid domain name, owner account ID, owner email, and optional description/tags
- **THEN** API Gateway SHALL validate the request structure and enqueue the command to the domains SQS queue
- **AND THEN** the API SHALL return `202 Accepted` with `{"requestId":"<SQS-MessageId>","message":"Domain registration request accepted"}`

#### Scenario: Registration command processed and succeeded event emitted

- **WHEN** the domain command handler processes a register-domain message from the queue
- **THEN** the handler SHALL validate business rules (naming, uniqueness)
- **AND THEN** read the platform-configured environment list
- **AND THEN** persist the domain in DynamoDB with status `PENDING_APPROVAL` and that environment list stamped onto the `environments` field of the domain record
- **AND THEN** start the approval Step Functions workflow
- **AND THEN** emit `RegisterDomainSucceeded` to the control plane event bus (including the stamped `environments` list)

#### Scenario: Registration command fails business validation

- **WHEN** the domain command handler processes a register-domain message and a business rule is violated (e.g., duplicate domain name, reserved name)
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

### Requirement: Describe Domain

Users SHALL be able to retrieve detailed information about a specific domain, including configuration, resources, and approval metadata. The `taskToken` field SHALL NOT be returned to domain owner callers. Platform team callers MAY access the `taskToken` when the domain status is `PENDING_APPROVAL`. Resource identifiers SHALL be returned as a map keyed by environment, where each entry contains the per-environment resource ARNs.

#### Scenario: Describe existing domain

- **WHEN** a user calls the describe-domain API with a valid domain identifier they are authorized to access
- **THEN** the control plane SHALL return domain configuration (name, description, owner, ownerEmail, tags, status, environments)
- **AND THEN** return resource identifiers as a map keyed by environment when the domain is `ACTIVE`; each entry SHALL contain `platformEventBusArn`, `appEventBusArn`, `schemaRegistryArn`, `subscriptionDlqArn`, `integrationDlqArn`, and `dashboardArn`
- **AND THEN** return domain-level identifiers (namespace parameter path, RAM share ARN) at the top level, not nested under any environment
- **AND THEN** return approval metadata (decidedBy, justification, approvedAt/rejectedAt) when available
- **AND THEN** the `taskToken` field SHALL NOT be included in the response for domain owner callers

#### Scenario: Describe unknown or unauthorized domain

- **WHEN** a user calls the describe-domain API with a domain identifier that does not exist or that they are not authorized to access
- **THEN** the control plane SHALL not leak domain existence details
- **AND THEN** return an error that represents either not-found or access-denied according to project security conventions

### Requirement: Domain management events

The domain management capability SHALL emit the following events. Each event SHALL have a well-defined type, a consistent payload, and a defined emitter (control plane command handler or workflow). Events SHALL be emitted on the control plane event bus. Command outcome events (`*Succeeded` / `*Failed`) are the primary mechanism by which callers learn the result of write operations. The approval workflow SHALL be the sole emitter of approval and rejection events.

#### Events emitted by the control plane command handler

| Event type | When emitted | Purpose | Minimum payload |
|------------|--------------|---------|-----------------|
| `RegisterDomainSucceeded` | After the register-domain command is successfully processed | Confirms domain registration was accepted | `domainId`, `domainName`, `ownerAccountId`, `ownerEmail`, `environments`, `status`, `createdAt` |
| `RegisterDomainFailed` | After the register-domain command fails (e.g., duplicate name, validation error) | Surfaces processing failure to callers and audit | `domainName`, `ownerAccountId`, `reason`, `failedAt` |
| `UpdateDomainSucceeded` | After the update-domain command is successfully processed | Confirms domain metadata update was applied | `domainId`, `updatedAt`, `changedFields` |
| `UpdateDomainFailed` | After the update-domain command fails | Surfaces processing failure to callers and audit | `domainId`, `reason`, `failedAt` |

#### Events emitted by the approval workflow

| Event type | When emitted | Purpose | Minimum payload |
|------------|--------------|---------|-----------------|
| `DomainRegistrationApproved` | After the approval workflow processes an approval decision | Triggers the provisioning workflow | `domainId`, `domainName`, `ownerAccountId`, `ownerEmail`, `environments`, `approvedBy`, `justification`, `approvedAt` |
| `DomainRegistrationRejected` | After the approval workflow processes a rejection decision | Triggers rejection notification; audit | `domainId`, `domainName`, `ownerAccountId`, `ownerEmail`, `rejectedBy`, `justification`, `rejectedAt` |

#### Events emitted by the provisioning workflow

| Event type | When emitted | Purpose | Minimum payload |
|------------|--------------|---------|-----------------|
| `DomainProvisioningSucceeded` | When the outer provisioning workflow completes successfully across all environments | Control plane updates domain to ACTIVE with per-environment resource ARNs | `domainId`, `workflowExecutionId`, `completedAt`, `environments`, `resourceArns` (map keyed by env), `namespaceParameter`, `isNewAccount` |
| `DomainProvisioningFailed` | When the outer provisioning workflow fails | Control plane updates domain to PROVISIONING_FAILED | `domainId`, `workflowExecutionId`, `failedAt`, `failedEnvironment`, `failedStep`, `errorCode`, `errorMessage`, `compensationErrors` |

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
