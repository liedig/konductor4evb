## Why

Nexus subscriptions currently rely on consumer-account ownership and globally visible schema discovery, which cannot express team-scoped consumer access or platform-admin-managed sharing. Subscriber role bindings and domain-owned producer entitlements are needed so teams can discover and consume only the schemas their consumer domains are authorized to use, with revocation taking effect immediately.

## What Changes

- Add platform-admin-managed subscriber role bindings from Cognito team groups or AWS-account principals to consumer domains, preserving OAuth interaction and IAM automation compatibility through common authorization decisions.
- Add two additive entitlement types owned by the consumer domain: all schemas from a producer domain, and one exact producer schema.
- Authorize subscription and catalog access only when the caller's Cognito group or IAM AWS account has the subscriber role on the consumer domain and that domain has a current covering entitlement.
- Disable active subscriptions after entitlement revocation when no other current grant covers them; re-evaluate all additive grants rather than associating a subscription with one grant.
- Add a subscriber-scoped schema catalog that expands current entitlements across environments and exposes list metadata or full schema detail.
- **BREAKING** Replace implicit account-ownership subscription authorization with explicit Cognito-group or AWS-account domain-role and entitlement checks; unauthorized or revoked resources return `404 Not Found`.
- **BREAKING** Replace the global subscription collection with domain-scoped create/list/update/delete routes, retain canonical `GET /subscriptions/{subscriptionId}`, and remove `consumerDomain` from mutable request bodies.
- **BREAKING** Replace globally visible schema discovery routes with consumer-domain catalog routes and subscriber authorization.
- Add inbound read/write and outbound read-only subscription views, with platform-wide inspection and `platform-auditor` read-only behavior.
- Preserve direct API Gateway-to-SQS integrations and `202 Accepted` semantics for subscription writes.
- Place resource-scoped subscription and catalog contracts on both separate OAuth and IAM surfaces; keep global platform administration and inspection on the OAuth surface.

## Capabilities

### New Capabilities

- `subscriber-entitlements`: Defines subscriber role bindings, consumer-domain producer entitlements, administration, additive authorization, revocation, and concealment behavior.

### Modified Capabilities

- `event-subscriptions`: Changes subscription ownership, authorization, lifecycle operations, and API routes to domain-scoped inbound and outbound views.
- `schema-management`: Replaces global schema discovery with an entitlement-expanded, consumer-domain catalog.
- `async-command-processing`: Changes subscription command routes and propagated authorization context while preserving direct SQS integration and asynchronous acceptance.

## Impact

The change affects Cognito claims, IAM AWS-account principals, authorization persistence and evaluation, subscription and schema catalog APIs, subscription command processing, EventBridge rule state, revocation workflows, and audit behavior. It explicitly depends on `add-cognito-access-foundation`, `evaluate-authorization-engine`, and `add-publisher-apis`; outbound publisher visibility depends on its publisher bindings, and the combined schema-management end state preserves its publisher-authorized schema creation and append APIs while replacing only global discovery. It does not add schema promotion, enterprise group mapping, or management of downstream application-bus rules.
