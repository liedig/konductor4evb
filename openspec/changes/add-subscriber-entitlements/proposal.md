## Why

Nexus subscriptions are currently authorized by matching the caller's AWS account to the consumer domain's owner, and schema discovery is globally visible to any authenticated caller. `add-grant-engine` delivers the bindings table and production `DynamoGrantEngine` (identity → domain role), and `add-publisher-apis` delivers publisher bindings and flat-verb schema writes. The missing piece is the second store: which consumer domains may consume which producer schemas. Consumer access must be explicit, additive, and revocable, with revocation stopping delivery on active subscriptions, not just new subscribe attempts.

## What Changes

- Add a dedicated entitlements table (store 2) holding consumer-domain → producer coverage grants of two additive types: domain-wide (all schemas from a producer domain, including future ones) and exact-schema (all versions of one producer schema, including future versions). Mutations use conditional writes with token-free audit records committed in the same transaction, mirroring the bindings table conventions.
- Extend both grant engine implementations so subscribe and enable actions compose two checks inside `ResourceAuthorizer.authorize()`: the caller's `subscriber` binding on the consumer domain (bindings table) and the consumer domain's covering entitlement (entitlements table). The composite travels through the trait's stringly `resource` parameter via a centralized grammar with typed constructor helpers; malformed resources fail closed. Self-subscription (consumer equals producer) is implicitly covered and requires only the binding check.
- Add flat-verb entitlement administration on the OAuth surface only: `POST /entitlements`, `GET /entitlements?consumerDomain=|producerDomain=`, `DELETE /entitlements/{entitlementId}`. Create and revoke are authorized by the `platform-admin` global role or a `publisher` binding on the entitlement's producer domain; lists additionally authorize a `subscriber` binding on the queried consumer domain. IAM-surface entitlement administration is a noted follow-up.
- Add an entitlement revocation worker. Revoke commits synchronously (conditional delete plus audit), so new subscribe and enable decisions deny immediately; the handler then enqueues an explicit SQS revocation message. A worker Lambda finds affected active subscriptions through a new producer reverse index on the event-streams table, re-evaluates each against all remaining current grants, and disables Nexus-managed EventBridge rules and per-environment desired state where no grant covers, with durable per-subscription progress and idempotent retries.
- Replace globally visible schema discovery with an entitlement-expanded consumer catalog: `GET /schemas` expands the queried consumer domain's current entitlements across environments, deduplicating overlapping grants against the per-environment registries without a catalog table. A caller with no entitlements sees an empty catalog. `platform-admin` and `platform-auditor` bypass the entitlement filter for platform-wide inspection (auditor read-only).
- **BREAKING** Replace account-ownership subscription authorization and path-scoped routes with flat-verb routes carrying `consumerDomain` in request bodies: `POST /subscriptions` (create), `POST /subscriptions/state` (per-environment enable/disable), `DELETE /subscriptions/{subscriptionId}` (unsubscribe), `GET /subscriptions?domain=&direction=inbound|outbound` (list), and canonical `GET /subscriptions/{subscriptionId}`. Unauthorized access returns `403 Forbidden`; deny-as-unknown concealment is dropped.
- Preserve direct API Gateway-to-SQS subscription writes with `202 Accepted`; the command handler re-authorizes through the engine before starting any workflow. Create and enable require a current covering entitlement; disable and delete remain available to an authorized subscriber after revocation. Subscription activation performs a final entitlement check before enabling any rule, and revocation never automatically re-enables.
- Delete the `IamOnlyAuthorizer` bridge once subscription command paths move to the grant engine.

## Capabilities

### New Capabilities

- `subscriber-entitlements`: Defines entitlement storage and types, administration authorization, additive coverage evaluation inside the grant engine, revocation workflow, and catalog visibility policy.

### Modified Capabilities

- `event-subscriptions`: Replaces account-ownership authorization with engine decisions, reshapes routes to flat verbs with inbound/outbound views, and adds the final entitlement check before rule enablement.
- `schema-management`: Replaces global schema discovery with the entitlement-filtered consumer catalog as the final visibility policy.
- `async-command-processing`: Reshapes subscription command routes and the propagated trusted-principal envelope while preserving direct SQS integration and `202 Accepted`.
- `control-plane-access`: Places entitlement administration on the OAuth surface only.

## Impact

This change depends on `add-grant-engine` (bindings table, engine implementations, action constants, `platform-admin` resolution) and `add-publisher-apis` (publisher bindings, flat-verb precedent, stage-based environments). It affects the entitlements table and its indexes, a producer reverse index on the event-streams table, both grant engine implementations and their fixture suites, the subscription command handler and API contracts on both surfaces, a new revocation queue and worker Lambda, the schema catalog read path, and the OAuth entitlement administration API. It owns the authorization-engine spike's deferred discover/revocation fan-out completion bound. It does not add schema promotion or compatibility design, IAM-surface entitlement administration, enterprise identity-provider group mapping, management of application-owned downstream bus rules, or automatic re-enablement after re-grant.
