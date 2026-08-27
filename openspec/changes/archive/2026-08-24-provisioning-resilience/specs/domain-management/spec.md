## ADDED Requirements

### Requirement: Provisioning retry for failed domains

A platform admin SHALL be able to retry provisioning for a domain in `PROVISIONING_FAILED` status via `POST /domains/{domainName}/state` with action `retry-provisioning` on the OAuth control-plane surface. The action SHALL conditionally transition the domain from `PROVISIONING_FAILED` to `APPROVED` (clearing `errorDetail`) and SHALL emit a `DomainProvisioningRetryRequested` lifecycle event on the control plane bus carrying the registration fields recorded on the domain record (`domainName`, `ownerAccountId`, `ownerEmail`, `secondaryOwnerEmails`, `environments`, plus the retrying principal and timestamp). The provisioning workflow SHALL start on that event exactly as it does for `DomainRegistrationApproved`. The retry SHALL fail with `409 Conflict` when the domain is not in `PROVISIONING_FAILED`; it SHALL NOT create, modify, or re-create role bindings (they already exist from the original approval). Team callers SHALL be denied with `403`. The IAM surface SHALL NOT expose retry in this capability version.

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
