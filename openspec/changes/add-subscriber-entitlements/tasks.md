## 1. Prerequisites and Authorization Composition

- [ ] 1.1 Confirm `add-grant-engine` and `add-publisher-apis` are complete; wire this change to their bindings table, `DynamoGrantEngine`/`InMemoryGrantEngine`, action constants, trusted-principal contract, publisher bindings, and flat-verb schema operations
- [ ] 1.2 Add composite resource grammar helpers in `nexus-common` next to the action constants: typed constructors for subscription resources (`{consumer}/{producer}/{event}`), the parser used by the engine, and the new action constants (`subscription:create`, `subscription:enable`, `subscription:disable`, `subscription:delete`); unit tests cover construction, round-trip parsing, and malformed input
- [ ] 1.3 Add typed entitlement models in `nexus-common`: consumer domain, producer domain, scope (domain-wide or exact-schema), schema name (exact-schema only), creation metadata, trusted actor; reject malformed scopes at the model level
- [ ] 1.4 Add behavior tests for OAuth-group and AWS-account domain isolation, IAM account-granularity across roles/sessions, equivalent OAuth/IAM decisions, exact versus domain-wide coverage, future-schema and future-version coverage, overlapping additive grants, self-subscription implicit coverage, and fail-closed malformed resources

## 2. Entitlement Storage

- [ ] 2.1 Add the `nexus-authorization-entitlements` DynamoDB table to the core stack: consumer-domain partition key, entitlement-id sort key, producer reverse GSI (`PRODUCER#{producerDomain}`), audit record item type, and retention policy matching the bindings table
- [ ] 2.2 Add entitlement access helpers in `nexus-common` (key construction, get/put/conditional delete, consumer-domain query, producer GSI query, transactional write with audit) with unit tests against the documented key patterns
- [ ] 2.3 Add a producer reverse index GSI on the event-streams table (`PRODUCER#{producerDomain}`, sort key carrying schema name) serving revocation candidate queries and outbound subscription views, with backfill or projection coverage for existing records

## 3. Grant Engine Extension

- [ ] 3.1 Extend `DynamoGrantEngine` to evaluate subscribe-family actions against both stores: binding check plus entitlement coverage for create/enable, binding-only for disable/delete and self-subscription, fail-closed on malformed composite resources, with structured token-free deny logging
- [ ] 3.2 Extend `InMemoryGrantEngine` with fixture-fed entitlements mirroring the same evaluation rules
- [ ] 3.3 Extend the shared decision-parity fixture suite with two-table cases (covering domain-wide grant, covering exact grant, overlapping grants, revoked grant, wrong consumer domain, self-subscription) and prove identical decisions on both implementations
- [ ] 3.4 Delete `IamOnlyAuthorizer` and rewire remaining consumers to the engine; update `nexus-common` doc comments

## 4. Entitlement Administration API

- [ ] 4.1 Add strict request/response models for create (consumerDomain, producerDomain, scope, optional schemaName), list (consumerDomain or producerDomain filter), and revoke (by entitlementId); tests prove callers cannot supply audit fields or identifiers
- [ ] 4.2 Add `POST /entitlements`, `GET /entitlements?consumerDomain=|producerDomain=`, and `DELETE /entitlements/{entitlementId}` to the OAuth OpenAPI contract only, wired to the REQUEST authorizer; contract tests prove the routes do not exist on the IAM surface
- [ ] 4.3 Implement the entitlement administration Lambda: dual authorization path (platform-admin, or publisher binding on the entitlement's producer domain; list additionally allows subscriber binding on the queried consumer domain), idempotent conditional create, conditional revoke with synchronous SQS revocation enqueue, 404/409 mapping, and one `TransactWriteItems` commit per mutation carrying its token-free audit record
- [ ] 4.4 Add handler tests for create/list/revoke under each authorization path, non-admin denial, producer self-service scoped to its own domain only, cross-producer denial, idempotent re-create, concurrent-revoke conflict mapping, audit contents, and revocation message contents

## 5. Subscription Commands

- [ ] 5.1 Reshape both surfaces' write contracts to flat verbs: `POST /subscriptions`, `POST /subscriptions/state`, `DELETE /subscriptions/{subscriptionId}` with `consumerDomain` in request bodies; update direct API Gateway-to-SQS mappings to inject the trusted-principal envelope (OAuth groups or IAM account) while preserving structural validation and `202 Accepted`
- [ ] 5.2 Change the subscription command handler to authorize through the engine before any workflow starts: create/enable require subscriber binding plus current covering entitlement; disable/delete require only the binding; domain `ACTIVE` checks, rule-name constraints, and self-subscription semantics are preserved
- [ ] 5.3 Perform a final current-entitlement check in the activation path immediately before any EventBridge rule transitions to `ENABLED`
- [ ] 5.4 Allow `platform-admin` to operate subscriptions without a subscriber binding while still requiring a current covering entitlement for create/enable
- [ ] 5.5 Add command behavior tests on OAuth and IAM paths for authorized create, missing binding, missing entitlement, alternative covering grants, post-revocation disable/delete, denied enable, platform-admin mutation, self-subscription, deterministic failure events, and idempotent retries

## 6. Entitlement Revocation Worker

- [ ] 6.1 Add the revocation SQS queue with its own DLQ, redrive policy, and retention matching the established command-queue pattern
- [ ] 6.2 Implement the revocation worker Lambda: bounded-batch candidate queries through the producer reverse index, per-candidate coverage re-evaluation through the engine, idempotent disable of Nexus-managed rules and per-environment desired state in every environment, and retention of rule identities, pinned versions, and subscription records
- [ ] 6.3 Persist durable per-subscription progress so a revocation completes only when every candidate is covered, disabled, or recorded in a durable failure state; retries re-read current grants and never emit unqualified success
- [ ] 6.4 Add worker tests for last-grant revocation, overlapping-grant preservation, revocation during activation, partial disable failure with durable recording, resumed retries with re-evaluation, idempotent disablement, and no mutation of downstream application-bus rules

## 7. Subscription Reads

- [ ] 7.1 Implement `GET /subscriptions?domain={domain}&direction=inbound|outbound` on both surfaces: inbound queries the consumer index under a subscriber binding; outbound queries the producer reverse index under a publisher binding with full consumer-topology visibility; support schema, version, and status filters and require direction
- [ ] 7.2 Implement canonical `GET /subscriptions/{subscriptionId}` authorization through the visible consumer side, producer side, or platform role, returning `403 Forbidden` for unauthorized callers and `404 Not Found` for missing subscriptions
- [ ] 7.3 Add read behavior tests for OAuth and IAM inbound/outbound visibility, publisher full outbound visibility, platform-wide inspection, `platform-auditor` read-only behavior, required direction, filtering, and 403/404 separation

## 8. Subscriber Schema Catalog

- [ ] 8.1 Implement `GET /schemas?consumerDomain={domain}`: read current entitlements, expand domain-wide grants against per-environment registries, look up exact-schema grants, deduplicate overlapping coverage, report latest version per environment, and apply producer-domain and environment filters with opaque cursor pagination
- [ ] 8.2 Implement `GET /schemas/{schemaName}?consumerDomain={domain}&domain={producerDomain}` with full latest content per environment under the same coverage check; uncovered targets return `403 Forbidden`
- [ ] 8.3 Implement the platform-role bypass: `platform-admin` and `platform-auditor` omit consumerDomain and receive the platform-wide catalog, auditor read-only
- [ ] 8.4 Add catalog behavior tests for grant union expansion, future schemas under a domain-wide grant, future versions under an exact-schema grant, exact-schema isolation, empty catalog for zero entitlements, environment skew, pagination across re-evaluated grants, revoked access, and platform bypass

## 9. Infrastructure and Operations

- [ ] 9.1 Provision the entitlements table, event-streams producer GSI, revocation queue and worker, and least-privilege IAM: administration Lambda gets conditional write plus transactional write on entitlements; the worker gets event-streams read/update and EventBridge rule disable only; consumers get none
- [ ] 9.2 Add metrics, structured audit logs, and alarms for authorization denials, revocation enqueue/progress/durable failures, failed rule disablement, command outcomes, and catalog latency without logging schema content, tokens, or claims
- [ ] 9.3 Add the seed path for migration: platform-admin tooling that creates subscriber bindings (via the grant-engine admin API) and entitlements for consumer domains retaining access before subscription re-evaluation, and verify rollback never bulk re-enables uncovered rules

## 10. Documentation and Validation

- [ ] 10.1 Update `docs/development/decisions.md` with the two-store model, composite resource grammar, dual-path entitlement administration, revocation transport and completion semantics, 403-over-404 denial, and `IamOnlyAuthorizer` deletion
- [ ] 10.2 Update `docs/development/architecture.md` with the entitlements table, engine composition, revocation worker flow, flat-verb subscription views, and catalog expansion
- [ ] 10.3 Update API and operational documentation with breaking route, request-body, authorization, and visibility migrations
- [ ] 10.4 Run focused crate tests during each red/green/refactor cycle, then `make check` and `make test` for the workspace
- [ ] 10.5 Run strict OpenSpec validation and verify the implementation satisfies every subscriber-entitlements, event-subscriptions, schema-management, async-command-processing, and control-plane-access scenario
