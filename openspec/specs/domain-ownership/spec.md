## Purpose

Domain ownership ties a domain to the Cognito team group that administers it. An `owner` role binding — established transactionally at registration approval and administered thereafter only by the platform team — scopes management reads and owner-permitted writes to the owning team's members. Because the owning team is also the domain's producer, approval SHALL additionally establish `publisher` and `subscriber` bindings for the same team in the same transaction; the `owner` role itself still does not subsume those roles.

## Requirements

### Requirement: Owner resource role

The grant engine SHALL recognize `owner` as a resource role alongside `publisher` and `subscriber`. An owner binding SHALL authorize domain-management operations scoped to its domain (metadata update, full-record reads) and SHALL NOT by itself authorize publishing, subscribing, entitlement granting, or platform operations. The role SHALL serialize as the lowercase token `owner` in keys and stored items.

#### Scenario: Owner binding authorizes domain management

- **WHEN** a caller's team group holds an `owner` binding on domain `orders`
- **THEN** owner-scoped operations on `orders` (metadata update, full-record describe) SHALL be authorized for that caller

#### Scenario: Owner role does not substitute for publisher or subscriber

- **WHEN** a caller's team group holds only an `owner` binding on `orders`
- **THEN** publish operations SHALL still require a `publisher` binding and subscribe operations SHALL still require a `subscriber` binding
- **AND THEN** the owner role alone SHALL NOT satisfy those checks

#### Scenario: Unknown roles remain unrepresentable

- **WHEN** role values are parsed from API input or storage
- **THEN** only `owner`, `publisher`, and `subscriber` SHALL be accepted
- **AND THEN** any other value SHALL fail validation rather than coerce

### Requirement: Ownership established at approval

When a domain registration that recorded an `owningTeam` is approved, the system SHALL create an owner binding from that issuer-qualified team group to the domain in the same transaction that commits the approval outcome, mirroring the transact-write pattern used for other state-plus-side-record commits. When a registration has no `owningTeam` (e.g. IAM-originated), approval SHALL create no owner binding and ownership administration falls to the platform team.

#### Scenario: Approval creates the owner binding atomically

- **WHEN** an approval workflow completes the accept path for a registration whose record carries `owningTeam: "team-payments"`
- **THEN** an owner binding for the issuer-qualified group `team-payments` on the domain SHALL be committed in the same transaction as the domain's approval state change
- **AND THEN** a failure of either write SHALL leave neither committed

#### Scenario: Approval without an owning team creates no binding

- **WHEN** an approval completes for a registration with no recorded `owningTeam`
- **THEN** no owner binding SHALL be created
- **AND THEN** the domain SHALL be manageable by platform roles until the platform team grants ownership explicitly

### Requirement: Only domain teams may own domains

The `owningTeam` recorded at registration SHALL be one of the requesting caller's Cognito team groups. Global platform groups (`platform-admin`, `platform-auditor`) SHALL NOT be acceptable as owning teams. Because trusted principals carry only non-global groups as team groups, a caller in zero team groups SHALL be unable to name a valid owning team.

#### Scenario: Platform group rejected as owning team

- **WHEN** a registration request names `platform-admin` as `owningTeam`
- **THEN** the request SHALL be denied as invalid
- **AND THEN** no domain record SHALL be persisted

#### Scenario: Caller must belong to the owning team

- **WHEN** a registration request names a team group the caller does not belong to
- **THEN** the request SHALL be denied
- **AND THEN** no domain record SHALL be persisted

### Requirement: Ownership transfer is platform-administered

Ownership changes after approval (adding or replacing the owning team) SHALL be performed by the platform team through the bindings API; owner teams SHALL NOT be able to transfer or relinquish ownership themselves.

#### Scenario: Owner cannot revoke its own ownership

- **WHEN** a caller whose only authorization is an owner binding attempts to delete that owner binding
- **THEN** the control plane SHALL deny the operation
- **AND THEN** the binding SHALL remain in place

#### Scenario: Platform admin re-homes ownership

- **WHEN** a `platform-admin` deletes the owner binding for `team-payments` on `orders` and creates one for `team-core-payments`
- **THEN** subsequent owner-scoped operations on `orders` SHALL be authorized for `team-core-payments` members and denied for `team-payments` members
