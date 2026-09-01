# domain-management Specification Delta

## MODIFIED Requirements

### Requirement: Update Domain

Domain owners SHALL be able to update mutable domain metadata by submitting a command that is processed asynchronously. The update command SHALL accept `description`, `ownerEmail`, `secondaryOwnerEmails`, and `costCenter` fields; all other domain attributes SHALL be immutable through this command. The domain name SHALL NOT be updatable. Status transitions SHALL NOT be performed via the update command. The `ownerAccountId` field SHALL NOT be updatable. The API path SHALL be `PUT /domains/{domainName}`. The API SHALL accept the command and return immediately; the outcome SHALL be communicated via an EventBridge event.

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

## ADDED Requirements

### Requirement: Domain resource tagging

Every domain SHALL carry a standard tag set, applied by the platform: `konductor:managed-by` (`konductor`), `konductor:domain` (the domain name), `konductor:environment` (per environment on per-environment resources), and `konductor:namespace` (the `{org}-{domain}` namespace identity). A registration MAY additionally supply a single governed custom tag, `costCenter`, for cost allocation; the API SHALL reject any other custom tag keys. The domain record SHALL store the effective tag set (standard set plus `cost-center` when supplied), and describe and list responses SHALL render it. `cost-center` SHALL be settable at registration and SHALL be mutable afterwards via the update command with change-only semantics: an update MAY set a `costCenter` where none exists and MAY replace an existing value with another valid value, but SHALL NOT clear an existing value (the API schema makes clearing inexpressible by requiring a non-empty value).

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

## REMOVED Requirements

### Requirement: Governed domain tag model

**Reason**: `cost-center` is no longer registration-only; it is mutable with change-only semantics via the update command. The requirement is superseded by "Domain resource tagging", which covers the same tag model plus mutability. The immutable-behavior scenario ("Cost center is immutable") cannot be carried through a MODIFIED delta without asserting the old behavior, so the requirement is replaced wholesale.

**Migration**: No client migration. `PUT /domains/{domainName}` gains an optional `costCenter` field; existing update payloads behave exactly as before.
