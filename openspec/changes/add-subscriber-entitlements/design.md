## Context

Subscriptions are currently authorized by matching the caller's AWS account to the consumer domain owner, and schema discovery is globally visible to authenticated callers. `add-cognito-access-foundation` introduces separate OAuth and IAM surfaces plus a common trusted-principal contract, `evaluate-authorization-engine` validates the selected DynamoDB grant persistence and policy implementation, and `add-publisher-apis` adds publisher domain bindings plus schema creation and append APIs. This design explicitly consumes all three prerequisites rather than reopening or replacing their decisions.

The change crosses the access-control, subscription, schema-management, API, and EventBridge materialization boundaries. Authorization must remain current after asynchronous API acceptance, overlapping grants must compose additively, and revocation must stop delivery without deleting the consumer domain's subscription contract.

## Goals / Non-Goals

**Goals:**

- Express subscriber access as a Cognito team-group or AWS-account role binding scoped to a consumer domain.
- Express producer sharing as consumer-domain-owned producer-domain and exact-schema grants.
- Apply one consistent, current authorization decision to subscription commands, subscription reads, and schema catalog reads.
- Disable no-longer-entitled active subscriptions immediately while preserving recoverable subscription state.
- Provide domain-scoped inbound and outbound subscription views and an entitlement-expanded schema catalog.
- Preserve direct API Gateway-to-SQS subscription writes and `202 Accepted` behavior.

**Non-Goals:**

- Publisher API creation or changes to the publisher-authorized schema creation, append, and event publication contracts delivered by `add-publisher-apis`.
- Schema registration, promotion, compatibility, or migration design.
- Enterprise identity-provider group mapping.
- Management of application-owned rules downstream of a consumer app-side bus.
- Reconsidering the DynamoDB authorization engine validated by the prerequisite spike.

## Decisions

### Keep authorization ownership separate from subscription ownership

The access-control bounded context owns role bindings, producer entitlements, and current authorization evaluation. Bindings target either a Cognito team group for OAuth or an AWS account for IAM, at account rather than IAM-role granularity. The subscription bounded context owns subscriptions by consumer domain and stores no creator or originating-grant ownership. Subscription and catalog services call the common authorization boundary with the normalized trusted principal, action, consumer domain, producer domain, and schema name.

This prevents a group rename, membership change, or grant replacement from changing resource ownership. Storing the grant that originally authorized a subscription was rejected because overlapping grants and later producer-domain grants would make revocation incorrect.

### Evaluate role and entitlement as a conjunction

Subscriber operations require both a current subscriber binding for at least one caller Cognito team group or the IAM caller's AWS account and a current grant owned by the consumer domain that covers the producer schema. Coverage is the union of exact-schema and producer-domain grants. Subscription disable and delete require the subscriber binding but deliberately do not require a current entitlement; enable and create require both. A global `platform-admin` may operate subscriptions without a subscriber binding, but create and enable still require the consumer domain to hold a current covering entitlement.

The authorization adapter uses the DynamoDB-backed grant engine validated by `evaluate-authorization-engine`. Both API surfaces normalize identity through the access-foundation `TrustedPrincipal` and invoke identical typed decisions rather than engine-specific policy or storage APIs. Global roles exist only for OAuth callers: `platform-admin` and `platform-auditor` may inspect all subscriptions, only `platform-admin` may administer bindings and entitlements, and `platform-auditor` remains read-only. IAM callers receive resource roles only through explicit AWS-account bindings.

### Place resource contracts on both access surfaces

All domain-scoped subscription routes, canonical subscription detail, and consumer catalog routes are exposed on both the OAuth and IAM REST APIs. OAuth handlers evaluate Cognito group bindings and optional global roles; IAM handlers evaluate AWS-account bindings while preserving the full IAM principal for audit. Shared handlers consume the same trusted-principal and authorization-decision contracts.

Global binding and entitlement administration and platform-wide inspection are OAuth-only because the access foundation defines `platform-admin` and `platform-auditor` as controlled Cognito global roles. Duplicating those global operations on IAM was rejected because IAM principals have no equivalent global-role contract.

### Re-authorize asynchronous commands in the command handler

API Gateway passes the access foundation's common trusted-principal context and route parameters in the direct SQS integration envelope on both surfaces. OAuth envelopes contain the Cognito subject, groups, and controlled global roles; IAM envelopes contain the AWS account and IAM principal ARN. Structural validation occurs before enqueueing, but the subscription command handler performs the current role-binding and entitlement decision before any workflow starts. The consumer domain is derived from the path-injected command envelope and is absent from mutable bodies.

Authorizing only at API acceptance was rejected because a queued command could execute after entitlement revocation. Replacing the direct integration with an authorization Lambda was rejected because it would change the established asynchronous boundary and `202 Accepted` contract.

### Re-evaluate coverage during entitlement revocation

Revocation durably records the grant as no longer current, then starts a revocation workflow for the affected consumer domain and grant scope. The workflow queries candidate active subscriptions and asks the authorization boundary whether each producer schema remains covered by any current grant. It disables all environments only when no alternative grant covers the schema, updates the per-environment desired state, and retains rule identities, pinned versions, and subscription records.

The revocation operation is not complete until every affected subscription is either confirmed covered, disabled when no longer covered, or recorded in a durable retry or failure state that identifies unfinished disablement. A partial EventBridge failure cannot produce an unqualified success. Retries re-read current additive grants and are idempotent, so a newly added alternative grant can legitimately remove the need to disable. The exact production completion bound is inherited from the authorization-engine spike recommendation; this change does not invent a fixed seconds-based SLO.

Subscription activation and enable workflows perform a final current-entitlement check before enabling EventBridge rules. This closes races where creation or enablement overlaps revocation. Revocation does not automatically re-enable subscriptions after a later grant; an authorized subscriber makes that operational choice.

Deleting rules on revocation was rejected because it would destroy pinned-version and topology state and would prevent subscribers from deleting or deliberately re-enabling the existing contract after access is restored.

### Expose domain-scoped subscription collections

The create and mutable routes are scoped by consumer domain:

- `POST /domains/{consumerDomain}/subscriptions`
- `PATCH /domains/{consumerDomain}/subscriptions/{subscriptionId}`
- `DELETE /domains/{consumerDomain}/subscriptions/{subscriptionId}`

The domain list route is `GET /domains/{domain}/subscriptions?direction=inbound|outbound` on both resource API surfaces. Inbound queries the consumer index and is authorized by subscriber access; outbound queries the producer partition and is authorized by publisher access. Both roles may derive from an OAuth team-group binding or an IAM AWS-account binding. Outbound returns complete subscription visibility because producer teams need consumer topology for notifications and future schema lifecycle/version migration. Canonical detail remains `GET /subscriptions/{subscriptionId}` and authorizes against either visible side or an OAuth global platform role.

Keeping a global list route was rejected because it obscures which domain role authorizes the query. Using event-stream terminology in the API was rejected in favor of the product term `subscriptions`.

### Conceal unauthorized details

Canonical subscription detail and schema catalog detail return the same `404 Not Found` response for a missing resource, missing role, or absent/revoked entitlement. List requests deny access without returning protected entries. This prevents schema and topology enumeration through distinguishable errors.

Returning `403 Forbidden` for an existing protected detail was rejected because it confirms resource existence.

### Build the schema catalog as an entitlement expansion

The schema-management service preserves the publisher-authorized schema creation and append-only version routes delivered by `add-publisher-apis`. This change replaces only global discovery: the consumer catalog reads current entitlements for the consumer domain, expands producer-domain grants against the producer registries, fetches exact-schema grants directly, and deduplicates by producer domain and schema name. List responses contain metadata and latest version by environment; detail responses contain full latest schema content by environment so version skew remains explicit. Producer-domain and environment filters are applied before expansion where possible.

Pagination uses an opaque continuation token that captures the normalized grant-expansion cursor and underlying registry cursor without exposing authorization data. Each page and detail request starts from current grants, so revocation takes effect without maintaining a duplicate catalog table.

Materializing a catalog table was rejected because registries remain the source of truth and producer-domain grants must automatically include future schemas.

### Preserve Nexus and application EventBridge boundaries

Nexus continues to manage only one rule per environment from the producer's platform-side domain bus to the consumer's app-side domain bus. Disable, enable, revocation, and deletion change only those Nexus-managed rules. Application rules attached downstream to the consumer bus remain application-owned.

## Risks / Trade-offs

- **[Revocation fan-out can be large]** → Query subscriptions through existing producer/consumer access paths, process candidates in bounded batches, make rule disablement idempotent, and emit observable per-subscription outcomes.
- **[Authorization can change during a catalog page sequence]** → Re-evaluate grants on every request and treat continuation tokens as cursors rather than authorization snapshots.
- **[Asynchronous callers receive `202` before authorization failure]** → Preserve the established contract and emit deterministic `*Failed` outcome events without retrying business failures.
- **[Concealed `404` responses reduce caller diagnostics]** → Record the internal denial reason in audit logs and outcome events without exposing it in detail responses.
- **[A revoked grant can overlap activation]** → Perform a final entitlement check before enabling rules and make the revocation workflow idempotently re-evaluate current coverage.
- **[Revocation can partially fail after some rules are disabled]** → Persist per-subscription progress and durable retry or failure state, never emit unqualified completion, and retry unfinished work idempotently after re-evaluating current grants.
- **[Breaking route cutover affects existing clients]** → Publish one coordinated OpenAPI cutover and remove mutable `consumerDomain` fields rather than supporting two conflicting ownership models.

## Migration Plan

1. Deploy `add-cognito-access-foundation`, the DynamoDB grant engine, and `add-publisher-apis`, including the publisher schema creation and append routes.
2. Add Cognito-group and AWS-account role bindings, entitlement administration, common authorization adapters, audit records, and subscription query access paths without changing public routes.
3. Have platform administrators create subscriber bindings and entitlements for consumer domains that must retain access.
4. Deploy revocation processing and re-evaluate existing active subscriptions; disable those with no current covering grant and durably record partial failures for retry.
5. Deploy the catalog and domain-scoped subscription read handlers on both OAuth and IAM surfaces without removing publisher schema writes.
6. Cut both resource API contracts and direct SQS mappings over to domain-scoped create, patch, and delete routes; remove the global list route and body-level `consumerDomain`.
7. Monitor authorization denials, revocation progress and durable failures, disabled-rule outcomes, command failures, and catalog latency against the bound selected by the engine spike.

Rollback SHALL preserve role bindings, entitlements, and any subscriptions disabled for lack of entitlement. The API deployment may be rolled back independently, but rollback SHALL NOT bulk re-enable rules or restore account-ownership authorization without an explicit security review.

## Open Questions

None. Authorization-engine readiness, OAuth/IAM access primitives, and publisher schema operations are prerequisite changes, not decisions owned by this change.
