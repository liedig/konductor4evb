## ADDED Requirements

### Requirement: Subscriber role binding

The production domain role-binding model consumed by this change SHALL support both `subscriber` and `publisher` bindings from either a Cognito team group for OAuth callers or an AWS account for IAM callers. A caller SHALL have subscriber access only when one of the caller's current Cognito groups or the caller's trusted IAM AWS account has a current `subscriber` binding to the consumer domain. Publisher authorization for outbound subscription visibility SHALL use the same principal alternatives and account granularity. Access SHALL belong to the bound group or account rather than to an individual user, IAM role, session, or subscription creator.

#### Scenario: Group member receives subscriber access

- **WHEN** a caller's current Cognito group has a current `subscriber` binding to consumer domain `checkout`
- **THEN** Nexus SHALL recognize the caller as a subscriber for `checkout`

#### Scenario: Binding does not cross domain boundaries

- **WHEN** a caller's group has a `subscriber` binding to `checkout` but not to `fulfilment`
- **THEN** Nexus SHALL NOT grant that caller subscriber access to `fulfilment`

#### Scenario: IAM account binding preserves automation compatibility

- **WHEN** a trusted IAM principal from AWS account `123456789012` calls a subscriber operation
- **AND WHEN** that AWS account has a current `subscriber` binding to consumer domain `checkout`
- **THEN** Nexus SHALL recognize the caller as a subscriber for `checkout` regardless of the IAM role or session name within that account

#### Scenario: IAM publisher binding grants outbound visibility

- **WHEN** a trusted IAM principal's AWS account has a current `publisher` binding to producer domain `orders`
- **THEN** Nexus SHALL recognize the caller as a publisher for `orders` outbound subscription reads at AWS-account granularity

### Requirement: Consumer-domain producer entitlements

Nexus SHALL store producer access as entitlements owned by a consumer domain, independently of users, Cognito groups, and AWS-account principals. Nexus SHALL support two additive grant types: a producer-domain grant covering every current and future schema in one producer domain, and an exact producer-schema grant covering one named schema in one producer domain. Grants SHALL apply logically across every environment in which the covered schema exists.

#### Scenario: Producer-domain grant covers current and future schemas

- **WHEN** consumer domain `checkout` has a current producer-domain grant for `orders`
- **THEN** every current schema in `orders` SHALL be entitled for `checkout`
- **AND WHEN** a new schema is later added to `orders`
- **THEN** that schema SHALL be entitled without changing the grant

#### Scenario: Exact-schema grant has narrow scope

- **WHEN** consumer domain `checkout` has an exact producer-schema grant for `orders` and `prezzee-orders@OrderNotification`
- **THEN** that schema SHALL be entitled in every environment where it exists
- **AND THEN** no other `orders` schema SHALL be entitled by that grant

### Requirement: Additive entitlement authorization

An operation requiring a subscriber entitlement SHALL be authorized only when the caller has subscriber access to the consumer domain through a current Cognito-group or IAM AWS-account binding and that consumer domain has at least one current producer-domain or exact producer-schema grant covering the requested schema. OAuth and IAM handlers SHALL invoke the same authorization decision contract after normalizing their distinct trusted-principal contexts. Nexus SHALL evaluate the union of all current grants at decision time and SHALL NOT associate continuing authorization with the grant that originally enabled an operation.

#### Scenario: Subscriber and covering grant authorize access

- **WHEN** a caller has subscriber access to `checkout`
- **AND WHEN** `checkout` has either a current producer-domain grant for `orders` or a current exact-schema grant for the requested `orders` schema
- **THEN** Nexus SHALL authorize the subscriber-entitled operation

#### Scenario: Role without entitlement is insufficient

- **WHEN** a caller has subscriber access to `checkout` but `checkout` has no current grant covering the requested schema
- **THEN** Nexus SHALL deny the subscriber-entitled operation

#### Scenario: Alternative grant preserves authorization

- **WHEN** one grant covering an existing subscription is revoked but another current grant still covers the same producer schema
- **THEN** Nexus SHALL continue to authorize that subscription

### Requirement: Platform administration and audit access

Only OAuth callers with the global `platform-admin` role SHALL create or revoke subscriber role bindings and producer entitlements. OAuth callers with the global `platform-admin` or `platform-auditor` role SHALL inspect role bindings, entitlements, subscriptions, and protected subscription topology across all domains. A platform administrator SHALL mutate subscriptions across domains subject to current entitlement rules. A `platform-auditor` SHALL NOT mutate role bindings, entitlements, or subscriptions. IAM callers SHALL receive no global platform-role bypass.

#### Scenario: Platform administrator manages an entitlement

- **WHEN** a caller with the global `platform-admin` role creates or revokes a producer entitlement
- **THEN** Nexus SHALL accept the authorized administration operation

#### Scenario: platform-auditor cannot mutate authorization state

- **WHEN** a caller whose only platform role is `platform-auditor` attempts to create or revoke a role binding or entitlement
- **THEN** Nexus SHALL reject the mutation

### Requirement: Separate OAuth and IAM resource surfaces

Domain-scoped subscription and consumer catalog routes SHALL be exposed consistently on both the OAuth and IAM API surfaces. OAuth requests SHALL derive group bindings and global roles from trusted Cognito authorizer context. IAM requests SHALL derive account bindings from the trusted AWS account in IAM request context. Both surfaces SHALL normalize their principal context and invoke the same resource-authorization decisions. Global binding and entitlement administration and platform-wide inspection SHALL be OAuth-only because their authorization depends on `platform-admin` or `platform-auditor`.

#### Scenario: Equivalent resource decision across surfaces

- **WHEN** an OAuth group binding and an IAM AWS-account binding grant `subscriber` access to the same consumer domain
- **THEN** equivalent domain-scoped subscription or catalog requests on their respective surfaces SHALL receive the same resource authorization decision

#### Scenario: IAM surface has no global administration route

- **WHEN** an IAM caller attempts to access global binding, entitlement administration, or platform-wide inspection
- **THEN** that operation SHALL NOT be exposed on the IAM API surface

### Requirement: Immediate entitlement revocation

Revoking an entitlement SHALL make that grant unavailable to new authorization decisions and trigger re-evaluation of every active subscription owned by the affected consumer domain and covered by the revoked grant. Nexus SHALL disable each active subscription for which no other current grant covers its producer schema, stopping delivery in every environment while preserving the subscription record, pinned versions, and EventBridge rules. The revocation operation SHALL NOT complete until every affected subscription has either been disabled when no longer covered, confirmed as covered by another current grant, or recorded in a durable retry or failure state that identifies the unfinished disablement. The exact production completion bound SHALL follow the authorization-engine spike recommendation rather than a fixed bound defined by this change.

#### Scenario: Last covering grant is revoked

- **WHEN** a platform administrator revokes the last current grant covering an active subscription
- **THEN** Nexus SHALL disable that subscription in every environment
- **AND THEN** retain its record, pinned versions, and EventBridge rules

#### Scenario: Overlapping grant prevents disablement

- **WHEN** a platform administrator revokes a grant covering an active subscription and another current grant covers the same producer schema
- **THEN** Nexus SHALL leave the subscription enabled

#### Scenario: Partial disable failure is durably tracked

- **WHEN** revocation disables some no-longer-covered subscriptions but one subscription cannot be disabled
- **THEN** the revocation operation SHALL NOT report unqualified successful completion
- **AND THEN** Nexus SHALL record a durable retry or failure state identifying the affected subscription and remaining work

#### Scenario: Retried revocation completes remaining disablement

- **WHEN** a durable revocation retry resumes after a partial failure
- **THEN** Nexus SHALL re-evaluate current additive coverage before retrying each unfinished disablement
- **AND THEN** it SHALL idempotently disable every subscription that remains no longer covered

### Requirement: Conceal unauthorized resources

Subscriber-scoped detail operations SHALL return `404 Not Found` when the caller lacks subscriber access, the required entitlement is absent or revoked, or the requested resource does not exist. The response SHALL NOT reveal which authorization condition failed.

#### Scenario: Revoked detail is concealed

- **WHEN** a caller requests subscriber-scoped detail after the relevant role binding or last covering entitlement has been revoked
- **THEN** Nexus SHALL return `404 Not Found`
- **AND THEN** the response SHALL NOT disclose whether the resource exists
