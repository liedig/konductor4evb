## Context

The `evaluate-authorization-engine` spike validated the DynamoDB grant model and directed a separate production change. Its readiness decision is **conditionally ready**, carrying residuals this change must address: in-region latency validation (#1), conditional writes for concurrent mutation (#2), and semantic audit items with time-ordered administration (#5). The access foundation already normalizes `TrustedPrincipal` for both surfaces and routes every consumer through the `ResourceAuthorizer` trait, so the engine has a stable seam to implement.

Publisher bindings and subscriber entitlements have different revocation semantics and different shipping schedules: publisher bindings gate publish/schema operations only (nothing materialized to reconcile), while entitlements gate subscriptions (materialized EventBridge rules to reconcile on revocation). They therefore live in separate stores and ship in separate changes; this change owns the bindings store and the engine both stores will use.

## Goals / Non-Goals

**Goals:**

- Production bindings table holding publisher and subscriber role bindings with conditional mutation writes and atomic token-free audit records.
- `DynamoGrantEngine` implementing `ResourceAuthorizer`: additive grants, default denial, strongly consistent decision reads, fail-closed on any error, structured internal deny-reason logging without changing the trait's `Allow`/`Deny` outcome.
- `InMemoryGrantEngine` fixture-fed twin with proven parity for tests and local development, reusing the spike's frozen fixture manifest as the seed suite.
- Flat-verb binding administration on the OAuth surface, `platform-admin`-only, with idempotent creates and durable revokes that take effect on the next decision.
- Authorizer renames aligned to function: `DenyAllAuthorizer`, `IamOnlyAuthorizer`.
- Development seed helper for straightforward local setup.
- In-region latency measurement confirming or refuting the spike's projected SLOs.

**Non-Goals:**

- Entitlements table, entitlement administration, entitlement evaluation, revocation worker, subscribe-path enforcement, and list-visibility policy (all `add-subscriber-entitlements`).
- Durable command-idempotency store (deferred to the first send-once change; existing handlers are state-idempotent and Step Functions task tokens are single-use).
- Deny-as-unknown response semantics (explicitly dropped; 403-vs-404 existence leakage is accepted at this stage).
- Typed action/resource parameters on the `ResourceAuthorizer` trait (kept stringly-typed for simplicity at this stage).
- Publish consumer and schema handler wiring (arrives with `add-publisher-apis`).

## Decisions

### Split bindings and entitlements into separate stores

This change ships one table, `nexus-authorization-bindings`, holding both publisher and subscriber role bindings:

```
RoleBinding    PK = PRINCIPAL#{cognito-group|aws-account}#{issuer-qualified id | account id}
               SK = ROLE#{publisher|subscriber}#DOMAIN#{domain}
               attrs: bindingId, state, createdAt, createdBy (trusted actor), provenance

AuditRecord    PK = GRANT#{bindingId}#AUDIT
               SK = TS#{timestamp}
               attrs: mutation type, actor, before/after summary (token-free)

GSI1 (admin):  PK = DOMAIN#{domain}, SK = ROLE#{role}#PRINCIPAL#{principal}
               enumeration by domain without scans

GSI2 (admin):  PK = bindingId
               point lookup for DELETE /bindings/{bindingId}, which carries
               no domain or principal context
```

Both GSIs are sparse: audit items carry no GSI attributes, so administration indexes contain bindings only. GSI2 was added during implementation — the revoke route identifies a binding by identifier alone, and the documented key schema has no access path from `bindingId` to the item's full key.

Entitlements ship later in their own table with their own admin GSI. Two stores, one engine crate: `DynamoGrantEngine` reads table 1 now and grows entitlement reads with the subscriber change. The split matches the revocation semantics (bindings: nothing to reconcile; entitlements: subscription reconciliation worker) and lets IAM policies scope publish consumers to the bindings table only. DynamoDB transactions span tables, so a future mutation touching both stores can still be atomic.

Subscriber authorization will need one item from each store (a subscriber binding from table 1 plus covering entitlement from table 2); that composition lands with the subscriber change. Storing both roles in table 1 keeps "who is this principal" separate from "what coverage does this domain have."

### Keep the trait stringly-typed with Allow/Deny outcomes

`ResourceAuthorizer` keeps `authorize(principal, action: &str, resource: &str) -> Allow | Deny`. Call sites stay readable, subscription tracking stays simple, and the engine interprets well-known action strings. Deny-as-unknown was considered for discover and explicitly dropped: denied and unknown both return ordinary errors, and existence leakage via 403-vs-404 is accepted at this stage. Deny reasons are emitted as structured internal logs by the engine (feeding the consumer warn-logging added by the foundation) without changing the outcome type. A future need for typed parameters or richer outcomes is an additive trait evolution, not a breaking change.

### Conditional writes and atomic audit for every mutation

Binding creates use `PutItem` with `attribute_not_exists` on the full key, making identical re-creates idempotent without partial state. Revokes use `DeleteItem` (or state transition) conditioned on the binding's existence and current state, so concurrent admin mutations cannot silently double-revoke or revive. Each mutation and its `AuditRecord` commit in one `TransactWriteItems` call: the audit trail cannot diverge from the grant state. Audit items carry mutation type, trusted actor, timestamp, and before/after summary; they never carry tokens, claims material, or principal ARNs beyond audit provenance. This addresses spike residuals #2 and #5.

Decisions always use strongly consistent reads on the bindings table. GSI1 is eventually consistent and serves administration enumeration only; it never feeds an allow decision (matching the spike's validated contract).

### Binding administration is flat-verb, OAuth-only, platform-admin-only

Routes on the OAuth surface only:

```
POST   /bindings                    { domain, role, principal: {type, id} }
GET    /bindings?domain={domain}    list bindings for a domain
DELETE /bindings/{bindingId}        revoke a binding
```

Only a normalized OAuth principal carrying the `platform-admin` global role may invoke them. The global role is resolved by the OAuth authorizer from controlled Cognito groups, never from the grant table, so administration cannot deadlock against an empty table. The IAM surface has no binding administration: IAM is for automation callers whose access is *governed by* bindings, not for governing them. Revocation is a durable delete that takes effect on the next decision; there is no reconciliation worker because publisher bindings have no materialized downstream state.

### Engine ships dark; publisher change is the first consumer

No existing consumer is rewired in this change. Domain-management and subscription paths keep `IamOnlyAuthorizer` (renamed from `FoundationAuthorizer`; behavior unchanged: IAM allow, OAuth deny). `add-publisher-apis` wires the publish consumer and schema handlers to `DynamoGrantEngine` as its first consumer. `add-subscriber-entitlements` later moves subscribe paths onto the engine and deletes `IamOnlyAuthorizer`. Shipping dark is safe because the spike already proved the decision contract and this change proves storage, mutation, administration, and audit through its own tests.

### In-memory twin and seed helper keep development straightforward

`InMemoryGrantEngine` implements the same trait against in-process fixtures, with a parity test suite proving identical decisions against both implementations using the spike's frozen fixture manifest (preserved under `evaluate-authorization-engine/evidence/`). Tests and local runs use the twin; cloud uses the DynamoDB engine. A seed helper (script or `make seed-bindings`) creates developer bindings for test Cognito groups and AWS accounts against a deployed environment, so no one clicks through the admin API by hand during development.

### Confirm latency SLOs in-region before publisher rollout

The spike's single readiness condition is in-region latency validation (residual #1). This change includes an in-region measurement (Lambda- or EC2-hosted benchmark against the deployed table in the production Region) compared against the spike's recommended SLOs: `schema:publish`-class decisions p50 ≤ 5 ms / p99 ≤ 20 ms. If the SLOs are missed, the spike's remediation ladder applies in order: co-location, read-count reduction on the hot path, short-TTL safe-cache for the binding lookup only (never entitlement/coverage checks, to preserve the revocation contract), then AVP reevaluation. The measurement gates the `add-publisher-apis` rollout, not this change's merge.

## Risks / Trade-offs

- [Engine ships dark with no production consumer] → Acceptable because the decision contract is spike-proven; this change's tests prove storage, mutation, admin, and audit, and `add-publisher-apis` provides the first live traffic.
- [Two stores mean the engine crate spans tables] → One crate, explicit per-store read paths; DDB transactions span tables if a future mutation needs atomicity across both.
- [Stringly-typed actions invite call-site typos] → Accept for now; action strings are centralized as constants in `nexus-common` so typos surface in tests, and the trait can evolve additively later.
- [Dropping deny-as-unknown leaks schema existence via 403-vs-404] → Accepted product decision at this stage; revisit only if schema existence becomes sensitive.
- [Conditional writes add retry complexity to admin mutations] → Condition-check failures map to deterministic 409 responses; no silent retries that could mask concurrent-admin races.
- [In-region latency could miss SLOs] → Spike remediation ladder is pre-agreed; measurement is a task in this change and a gate on publisher rollout.
- [GSI lag could show a revoked binding briefly in admin lists] → Administration reads tolerate eventual consistency; decisions never use the GSI, so authorization is unaffected.

## Migration Plan

1. Provision the bindings table, GSI, and least-privilege IAM in the core stack.
2. Land the engine crate: binding models, `DynamoGrantEngine`, `InMemoryGrantEngine`, parity fixtures from the spike manifest.
3. Rename `FailClosedAuthorizer` → `DenyAllAuthorizer` and `FoundationAuthorizer` → `IamOnlyAuthorizer` across consumers (behavior unchanged).
4. Add binding administration routes and Lambda; create initial developer bindings via the admin API and the seed helper.
5. Run the in-region latency benchmark and record results against the spike SLOs.
6. Update `add-publisher-apis` tasks to consume `DynamoGrantEngine` (removing its authorization-adapter prerequisite).

Rollback: remove the admin routes and Lambda, delete the table, revert the renames. No consumer behavior changes at any point, so rollback cannot break existing callers. Bindings are dev-stage data; no data migration is required.

## Open Questions

None. Entitlement composition, the revocation worker, list visibility, and the command-idempotency store are explicitly deferred with named owners (`add-subscriber-entitlements`; first send-once change).
