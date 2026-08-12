## ADDED Requirements

### Requirement: Binding storage holds typed principal role bindings

Konductor SHALL store role bindings in a dedicated bindings table keyed by typed principal and domain. A binding principal SHALL be exactly one of an issuer-qualified Cognito team group or a twelve-digit AWS account. A binding SHALL grant exactly one resource role (`publisher` or `subscriber`) on exactly one domain. The store SHALL NOT represent a global (domain-less) resource-role binding. Every binding SHALL carry a stable binding identifier, state, creation metadata, and the trusted actor that created it.

#### Scenario: Cognito team-group binding is stored

- **WHEN** a platform-admin binds issuer-qualified group `team-orders` to role `publisher` on domain `orders`
- **THEN** the store SHALL contain one RoleBinding keyed by that typed principal, role, and domain
- **AND THEN** the binding SHALL carry a stable identifier, state, creation metadata, and trusted actor

#### Scenario: AWS-account binding is stored

- **WHEN** a platform-admin binds AWS account `111122223333` to role `publisher` on domain `orders`
- **THEN** the store SHALL contain one RoleBinding keyed by that typed principal, role, and domain
- **AND THEN** the binding SHALL be independent of any IAM role, user, or session ARN within that account

#### Scenario: Global resource binding is rejected

- **WHEN** a request attempts to bind a principal to a resource role without a domain
- **THEN** the request SHALL be rejected
- **AND THEN** no binding SHALL be created

### Requirement: Additive grant evaluation with default denial

The grant engine SHALL evaluate a normalized `TrustedPrincipal` against an action and resource and return only `Allow` or `Deny`. Applicable bindings SHALL form an additive union: any single applicable active binding allows the action, and the absence of any applicable binding denies it. The engine SHALL deny on any internal error (fail closed). OAuth principals SHALL be matched by issuer-qualified team groups; IAM principals SHALL be matched by AWS account ID, and the principal ARN SHALL NOT select grants. A binding for one domain SHALL NOT authorize another domain.

#### Scenario: Team-group binding authorizes its domain

- **WHEN** an OAuth caller belongs to a team group with an active `publisher` binding for domain `orders`
- **THEN** the engine SHALL allow publisher actions for `orders`

#### Scenario: AWS-account binding authorizes its domain

- **WHEN** an IAM caller's authenticated account has an active `publisher` binding for domain `orders`
- **THEN** the engine SHALL allow publisher actions for `orders` regardless of IAM role, user, or session provenance

#### Scenario: Binding is domain-isolated

- **WHEN** a caller has a `publisher` binding for `orders` but requests a publisher action for `payments`
- **THEN** the engine SHALL deny

#### Scenario: Overlapping bindings are additive

- **WHEN** a caller has two bindings that each independently cover an action and one is revoked
- **THEN** the engine SHALL continue to allow through the remaining binding

#### Scenario: Engine failure denies

- **WHEN** the engine cannot read binding state for a decision
- **THEN** the engine SHALL return `Deny`
- **AND THEN** the engine SHALL NOT allow the action

### Requirement: Strongly consistent decision reads

Authorization decisions SHALL use strongly consistent reads against the bindings table. Administration enumeration MAY use the admin GSI with eventual consistency, but GSI results SHALL NOT feed an allow decision.

#### Scenario: Revocation takes effect on the next decision

- **WHEN** a binding is revoked and a subsequent decision is evaluated for the same principal, action, and domain
- **THEN** the decision SHALL reflect the revocation immediately through strongly consistent reads

#### Scenario: Admin list reflects GSI consistency only

- **WHEN** a binding is created or revoked and an administrator lists bindings immediately after
- **THEN** the list MAY briefly lag
- **AND THEN** authorization decisions SHALL NOT be affected by that lag

### Requirement: Conditional mutation writes with atomic audit

Binding mutations SHALL use conditional writes: creates SHALL be idempotent for the same typed principal, role, and domain via a not-exists condition, and revokes SHALL be conditioned on the binding's current existence and state. Every mutation SHALL commit its token-free audit record in the same transaction as the mutation itself. Audit records SHALL carry mutation type, trusted actor, timestamp, and before/after summary, and SHALL NOT carry tokens, claims material, or IAM principal ARNs beyond audit provenance.

#### Scenario: Identical re-create is idempotent

- **WHEN** a platform-admin creates a binding identical to an existing active binding
- **THEN** the operation SHALL succeed without creating a duplicate binding
- **AND THEN** exactly one active binding SHALL exist for that typed principal, role, and domain

#### Scenario: Concurrent revoke conflicts deterministically

- **WHEN** two revoke operations target the same binding concurrently
- **THEN** at most one SHALL succeed
- **AND THEN** the other SHALL fail with a deterministic conflict error
- **AND THEN** the binding SHALL remain revoked exactly once

#### Scenario: Mutation and audit commit atomically

- **WHEN** any binding mutation commits
- **THEN** its audit record SHALL exist in the same committed transaction
- **AND THEN** a failed mutation SHALL NOT leave an audit record, and a failed audit SHALL NOT leave a mutation

### Requirement: In-memory evaluator parity

Konductor SHALL provide an in-memory engine implementing the same `ResourceAuthorizer` contract for tests and local development. A shared fixture suite, seeded from the authorization-engine spike's frozen fixture manifest, SHALL prove identical decisions between the in-memory and DynamoDB implementations.

#### Scenario: Fixture suite passes on both engines

- **WHEN** the shared decision fixtures execute against the in-memory engine and the DynamoDB engine
- **THEN** every fixture SHALL produce the same decision on both implementations

#### Scenario: Local development requires no AWS credentials

- **WHEN** tests or local runs exercise resource authorization
- **THEN** the in-memory engine SHALL evaluate decisions without AWS credentials or network access

### Requirement: Platform-admin binding administration

The OAuth control-plane surface SHALL expose binding administration as flat-verb routes: `POST /bindings`, `GET /bindings?domain=…`, and `DELETE /bindings/{bindingId}`. Every administration route SHALL require a normalized OAuth principal carrying the `platform-admin` global role. The `platform-admin` role SHALL be resolved by the OAuth authorizer from controlled Cognito groups and SHALL NOT be read from the bindings table, so administration cannot deadlock against an empty table. The IAM surface SHALL NOT expose binding administration.

#### Scenario: Platform-admin creates a binding

- **WHEN** an OAuth caller with the `platform-admin` global role creates a valid binding
- **THEN** the API SHALL create the binding and return its identifier
- **AND THEN** a token-free audit record SHALL be committed with the mutation

#### Scenario: Non-admin caller is denied

- **WHEN** an OAuth caller without the `platform-admin` global role invokes any binding administration route
- **THEN** the API SHALL return `403 Forbidden`
- **AND THEN** no mutation SHALL occur

#### Scenario: IAM surface has no binding administration

- **WHEN** any caller requests binding administration on the IAM control-plane surface
- **THEN** no such route SHALL exist

#### Scenario: Publisher cannot administer its own bindings

- **WHEN** a caller holding only a `publisher` binding attempts binding administration
- **THEN** the API SHALL return `403 Forbidden`
