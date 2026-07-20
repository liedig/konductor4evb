# Capability: Domain Management (delta)

## MODIFIED Requirements

### Requirement: Register Domain

Domain management is an administrative function of the control plane. Domain owners SHALL be able to request registration of a new domain by submitting a command that is processed asynchronously. The API SHALL accept the command and return immediately with a request ID (SQS message ID) and a descriptive message. Each request SHALL go through an approval process; only after approval SHALL the provisioning workflow be triggered. All domain details SHALL be stored in the single domains DynamoDB table. The domain name SHALL be used as the DynamoDB partition key. Uniqueness SHALL be enforced atomically via a conditional `PutItem` with `attribute_not_exists(domainName) OR #status = :REJECTED` — the system SHALL NOT perform a pre-write query to check for duplicates. A `REJECTED` record SHALL NOT block its domain name: a new registration for that name SHALL atomically replace the rejected record with a fresh registration (new `requestId`, status `PENDING_APPROVAL`, fresh approval cycle); rejection history remains available through the `DomainRegistrationRejected` events on the control plane bus.

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
