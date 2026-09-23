# Production grant engine: bindings table, dual engines behind `ResourceAuthorizer`, OAuth-only binding administration—shipped dark

[Documentation home](../README.md)

- Recorded date: 2026-08-03
- Change ID: `add-grant-engine`
- Area: Authorization
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

**Two-store split**: publisher/subscriber role bindings live in `konductor-authorization-bindings` now; subscriber entitlements get their own table in `add-subscriber-entitlements`. Different revocation semantics (bindings gate nothing materialized; entitlements gate subscriptions needing reconciliation) and different shipping schedules; DynamoDB transactions span tables, so a future cross-store mutation can still be atomic.

**Stringly-typed trait retained**: the existing `ResourceAuthorizer` seam (`authorize(&TrustedPrincipal, action, resource) -> Allow\|Deny`) stays—typed actions/resources would churn every consumer for no decision-quality gain; action strings are centralized constants in `konductor-common` so typos surface in tests.

**Deny-as-unknown dropped**: the engine returns `Deny`; 404-vs-403 response shaping is an API-layer concern, and the spike's deny-as-unknown polish added ambiguity without security value.

**No revocation worker**: publisher bindings have no materialized downstream state, so revoke is a durable conditional delete effective on the next decision—no reconciliation machinery (that arrives with entitlements).

**Conditional writes + atomic audit**: creates use `attribute_not_exists` puts (idempotent re-create answers the stored binding; concurrent-revoke races map to 409), revokes use existence/state-conditioned deletes, and every mutation commits with its token-free `AuditRecord` in one `TransactWriteItems` so the audit trail cannot diverge from grant state. **Decisions are strongly consistent point reads**; the admin GSIs (`by-domain`, `by-binding-id`) are sparse, eventually consistent, and never feed an allow decision. **Binding administration is flat-verb, OAuth-only, platform-admin-only** (`POST/GET/DELETE /bindings`): the platform-admin role resolves from the controlled Cognito group, never the grant table, so administration cannot deadlock against an empty table; the IAM surface has no binding routes.

**Engines ship as a pair**: `DynamoGrantEngine` (cloud) and `InMemoryGrantEngine` (fixtures) behind the same trait, with a manifest-seeded parity suite proving identical decisions; no existing consumer is rewired (dark ship)—`IamOnlyAuthorizer` (renamed from `FoundationAuthorizer`, with `FailClosedAuthorizer` → `DenyAllAuthorizer`) keeps guarding current paths until `add-publisher-apis` and `add-subscriber-entitlements` migrate them.

**Model hardening**: `BindingPrincipal` deserialization runs the validating constructors, so malformed principals are unrepresentable via API bodies as well as code; global (domain-less) bindings are rejected at the model level.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-08-05-add-grant-engine/proposal.md). The recorded date above is not necessarily the archive or merge date.

[Decision index](README.md)
