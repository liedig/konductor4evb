# Capability: Subscriber Entitlements

## Purpose

How the platform grants and revokes consumer-domain access to producer event contracts: an entitlement store holding additive coverage grants, dual-authorized administration, coverage evaluation inside the grant engine, and the revocation pipeline that converges existing subscriptions when coverage is withdrawn.

## Requirements

### Requirement: Entitlement storage holds consumer-domain coverage grants

Nexus SHALL store entitlements in a dedicated entitlements table keyed by consumer domain with a stable entitlement identifier. An entitlement SHALL grant a consumer domain coverage of the producer schemas matched by exactly one scope pattern: a string in the EventBridge schema-name charset with an optional single trailing `*`. The pattern `*` SHALL cover all schemas from the producer domain including schemas created later; an exact name SHALL cover all versions of one schema including versions published later; a name prefix with a trailing `*` SHALL cover the matching family including matching schemas created later. Malformed patterns SHALL be rejected before any write. Every entitlement SHALL carry consumer domain, producer domain, scope pattern, creation metadata, and the trusted actor that created it. Entitlements SHALL be environment-agnostic: coverage SHALL be evaluated identically regardless of the environment the consumer works in, and per-environment control SHALL live in the subscription rather than the entitlement. Revocation SHALL delete the entitlement row; revoked history SHALL exist only in audit records, so current coverage is always an existence check.

#### Scenario: Domain-wide pattern covers future schemas

- **WHEN** a consumer domain holds an entitlement with scope pattern `*` for producer domain `orders`
- **AND WHEN** a publisher later creates a new schema in `orders`
- **THEN** the new schema SHALL be covered without any entitlement change

#### Scenario: Exact-name pattern covers future versions

- **WHEN** a consumer domain holds an entitlement whose scope pattern is the exact name of one producer schema
- **AND WHEN** a publisher later appends a new version to that schema
- **THEN** the new version SHALL be covered without any entitlement change

#### Scenario: Prefix pattern covers a family including future members

- **WHEN** a consumer domain holds an entitlement with scope pattern `Order*` for producer domain `orders`
- **THEN** existing and later-created `orders` schemas whose names begin with `Order` SHALL be covered
- **AND THEN** schemas whose names do not begin with `Order` SHALL NOT be covered

#### Scenario: Malformed pattern rejected

- **WHEN** an entitlement create carries a scope pattern outside the schema-name charset or with a wildcard other than a single trailing `*`
- **THEN** the request SHALL be rejected and no entitlement SHALL be created

#### Scenario: Coverage is environment-agnostic

- **WHEN** a subscription is created under a covering entitlement with per-environment directives
- **THEN** the entitlement SHALL cover every environment's directive identically
- **AND THEN** per-environment versions and enablement SHALL be governed by the subscription alone

#### Scenario: Revoked entitlement no longer covers

- **WHEN** an entitlement is revoked
- **THEN** the entitlement row SHALL cease to exist
- **AND THEN** subsequent coverage checks SHALL deny, with history available only in audit records

### Requirement: Entitlement administration is dual-authorized on the OAuth surface

Entitlement administration SHALL be exposed as flat-verb routes on the OAuth control-plane surface only: `POST /entitlements`, `GET /entitlements?consumerDomain=|producerDomain=`, and `DELETE /entitlements/{entitlementId}`. Create and revoke SHALL be authorized by the `platform-admin` global role or a `publisher` binding on the entitlement's producer domain. List SHALL additionally authorize a `subscriber` binding matching the queried consumer domain. The IAM surface SHALL NOT expose entitlement administration in this capability version.

#### Scenario: Producer self-service sharing

- **WHEN** a caller with a `publisher` binding for producer domain `orders` creates an entitlement covering `orders` schemas for a consumer domain
- **THEN** the API SHALL create the entitlement and commit its audit record with the mutation

#### Scenario: Producer cannot share another domain's schemas

- **WHEN** a caller with a `publisher` binding for `orders` attempts to create an entitlement covering producer domain `payments`
- **THEN** the API SHALL return `403 Forbidden`
- **AND THEN** no entitlement SHALL be created

#### Scenario: Platform-admin administers any entitlement

- **WHEN** an OAuth caller with the `platform-admin` global role creates, lists, or revokes entitlements for any domain pair
- **THEN** the API SHALL perform the operation

#### Scenario: Subscriber lists its own consumer entitlements

- **WHEN** a caller with a `subscriber` binding on consumer domain `checkout` lists entitlements filtered by `consumerDomain=checkout`
- **THEN** the API SHALL return that consumer domain's current entitlements

#### Scenario: IAM surface has no entitlement administration

- **WHEN** any caller requests entitlement administration on the IAM control-plane surface
- **THEN** no such route SHALL exist

### Requirement: Entitlement mutations are conditional with atomic audit

Entitlement creates SHALL be idempotent for the same consumer domain, producer domain, and scope via a not-exists condition. Revokes SHALL be conditioned on the entitlement's current existence. Every mutation SHALL commit a token-free audit record (mutation type, trusted actor, timestamp, scope summary) in the same transaction as the mutation itself. A committed revoke SHALL result in exactly one revocation message carrying the full revoked scope being enqueued to the revocation queue, produced from the entitlements table stream so the notification is atomic with the delete and cannot be lost by a post-commit handler failure. Duplicate stream deliveries SHALL NOT produce duplicate revocation messages. A committed revoke SHALL also emit an `EntitlementRevoked` business event on the control-plane bus.

#### Scenario: Identical re-create is idempotent

- **WHEN** an identical active entitlement already exists for a create request
- **THEN** the operation SHALL succeed without creating a duplicate
- **AND THEN** exactly one entitlement SHALL exist for that consumer, producer, and scope

#### Scenario: Mutation and audit commit atomically

- **WHEN** any entitlement mutation commits
- **THEN** its audit record SHALL exist in the same committed transaction
- **AND THEN** a failed mutation SHALL NOT leave an audit record, and a failed audit SHALL NOT leave a mutation

#### Scenario: Revoke enqueues the revocation workflow

- **WHEN** an entitlement revoke commits
- **THEN** exactly one revocation message carrying the revoked consumer domain, producer domain, and scope SHALL be enqueued for asynchronous subscription re-evaluation

#### Scenario: Revocation notification survives handler failure

- **WHEN** an entitlement revoke commits
- **AND WHEN** the administration handler fails before returning a response
- **THEN** the revocation message SHALL still be enqueued via the entitlements table stream
- **AND THEN** an `EntitlementRevoked` business event SHALL still be emitted

### Requirement: Subscribe authorization composes binding and coverage inside the grant engine

The grant engine SHALL evaluate subscribe-family actions against both authorization stores inside a single `authorize()` call, addressed through a composite resource of the form `{consumerDomain}/{producerDomain}/{eventName}` constructed by typed helpers. For `subscription:create` and `subscription:enable`, the engine SHALL allow only when the caller holds a `subscriber` binding on the consumer domain AND at least one current entitlement covers the producer schema; overlapping entitlements SHALL compose additively. For `subscription:disable` and `subscription:delete`, the engine SHALL require only the `subscriber` binding so subscribers retain lifecycle control after revocation. Self-subscription (consumer equals producer) SHALL be implicitly covered and require only the binding check. Malformed composite resources SHALL deny. Callers SHALL NOT compose binding and entitlement checks outside the engine.

#### Scenario: Covered subscribe is allowed

- **WHEN** a caller holds a `subscriber` binding on consumer domain `checkout` and `checkout` holds a covering entitlement for the target producer schema
- **THEN** the engine SHALL allow `subscription:create`

#### Scenario: Missing entitlement denies despite binding

- **WHEN** a caller holds a `subscriber` binding on `checkout` but no current entitlement covers the target producer schema
- **THEN** the engine SHALL deny
- **AND THEN** the API SHALL return `403 Forbidden`

#### Scenario: Alternative covering grant preserves access

- **WHEN** a consumer domain holds two overlapping entitlements covering a schema and one is revoked
- **THEN** the engine SHALL continue to allow subscribe and enable actions for that schema through the remaining entitlement

#### Scenario: Post-revocation lifecycle control retained

- **WHEN** an entitlement covering an existing subscription has been revoked
- **THEN** an authorized subscriber SHALL remain able to disable or delete the subscription
- **AND THEN** enable SHALL be denied

#### Scenario: Self-subscription is implicitly covered

- **WHEN** a caller with a `subscriber` binding on a domain subscribes that domain to one of its own schemas
- **THEN** the engine SHALL allow without any entitlement

#### Scenario: Malformed composite resource denies

- **WHEN** the engine receives a subscribe-family action with a resource that does not parse as `{consumerDomain}/{producerDomain}/{eventName}`
- **THEN** the engine SHALL deny and log the internal reason without tokens, claims, or payloads

### Requirement: Entitlement revocation disables uncovered subscriptions asynchronously

Revocation SHALL take effect for new decisions synchronously with the entitlement delete. A revocation worker SHALL then process the queued revocation message in bounded batches: query candidate active subscriptions through the producer reverse index, re-evaluate each candidate's producer schema against the consumer domain's remaining current grants through the engine, and where no grant covers, disable the Nexus-managed EventBridge rule and per-environment desired state in every environment. The worker SHALL retain subscription records, rule identities, and pinned versions. Disablement SHALL be idempotent, and every retry SHALL re-read current grants. Worker failures SHALL be retried through the queue's redrive policy and surfaced via the dead-letter queue. A scheduled reconciliation sweep SHALL re-evaluate active subscriptions against current coverage and disable any that remain uncovered. Revocation SHALL NOT automatically re-enable subscriptions after a later grant.

#### Scenario: Uncovered subscription is disabled in every environment

- **WHEN** the last entitlement covering an active subscription is revoked and the worker processes the message
- **THEN** the worker SHALL disable the Nexus-managed rule and desired state in every environment
- **AND THEN** the subscription record, rule identities, and pinned versions SHALL be retained

#### Scenario: Alternative grant cancels disablement

- **WHEN** a candidate subscription remains covered by another current entitlement at evaluation time
- **THEN** the worker SHALL leave it enabled

#### Scenario: Worker failure retries and surfaces

- **WHEN** disabling fails for a subset of candidates
- **THEN** the message SHALL be retried through the queue's redrive policy with current grants re-read
- **AND THEN** a message that exhausts retries SHALL land in the dead-letter queue with an alarm

#### Scenario: Reconciliation sweep disables uncovered drift

- **WHEN** an active subscription has no current covering entitlement
- **AND WHEN** the scheduled reconciliation sweep runs
- **THEN** the sweep SHALL disable the subscription's Nexus-managed rule and desired state in every environment

#### Scenario: Retry honors a newly added grant

- **WHEN** a retry runs after a new covering entitlement was created for a previously uncovered candidate
- **THEN** the retry SHALL evaluate the candidate as covered and skip disablement

#### Scenario: No automatic re-enablement

- **WHEN** a new entitlement is created after subscriptions were disabled for lack of coverage
- **THEN** those subscriptions SHALL remain disabled until an authorized subscriber deliberately enables them

### Requirement: Schema catalog visibility follows entitlement coverage

The consumer schema catalog SHALL expose exactly the schemas covered by the queried consumer domain's current entitlements, expanded across environments against the per-environment registries with overlapping coverage deduplicated. A consumer domain with no current entitlements SHALL receive an empty catalog list. Every catalog page and detail request SHALL re-evaluate current entitlements; continuation tokens SHALL be cursors only and SHALL NOT snapshot authorization state. `platform-admin` and `platform-auditor` SHALL bypass the entitlement filter for platform-wide inspection, with `platform-auditor` read-only. No catalog table SHALL be introduced; registries remain the source of truth.

#### Scenario: Catalog expands current entitlements

- **WHEN** a caller with a `subscriber` binding on `checkout` requests the catalog for `checkout`
- **THEN** the response SHALL contain exactly the schemas matched by `checkout`'s current entitlement scope patterns, deduplicated, with latest version per environment

#### Scenario: Zero entitlements yields an empty catalog

- **WHEN** a caller queries the catalog for a consumer domain with no current entitlements
- **THEN** the response SHALL be an empty list
- **AND THEN** the response SHALL NOT indicate which producer domains or schemas exist

#### Scenario: Revocation takes effect on the next catalog request

- **WHEN** an entitlement is revoked between two catalog requests
- **THEN** the second request SHALL exclude the previously covered schemas

#### Scenario: Platform roles inspect the full catalog

- **WHEN** a `platform-admin` or `platform-auditor` caller requests the catalog without a consumer domain
- **THEN** the response SHALL span all discoverable schemas unfiltered by entitlements
- **AND THEN** `platform-auditor` SHALL NOT be able to perform any mutation
