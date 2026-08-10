## 1. Prerequisites and Authorization Composition

- [x] 1.1 Confirm `add-grant-engine` and `add-publisher-apis` are complete; wire this change to their bindings table, `DynamoGrantEngine`/`InMemoryGrantEngine`, action constants, trusted-principal contract, publisher bindings, and flat-verb schema operations
- [x] 1.2 Add composite resource grammar helpers in `nexus-common` next to the action constants: typed constructors for subscription resources (`{consumer}/{producer}/{event}`), the parser used by the engine, and the new action constants (`subscription:create`, `subscription:enable`, `subscription:disable`, `subscription:delete`); unit tests cover construction, round-trip parsing, and malformed input
- [x] 1.3 Add typed entitlement models in `nexus-common`: consumer domain, producer domain, scope pattern (schema-name charset with an optional single trailing `*`, validated at construction), creation metadata, trusted actor; reject malformed patterns at the model level
- [x] 1.4 Add behavior tests for OAuth-group and AWS-account domain isolation, IAM account-granularity across roles/sessions, equivalent OAuth/IAM decisions, exact-name, domain-wide `*`, and prefix coverage, future-schema and future-version coverage (including prefix families), overlapping additive grants, self-subscription implicit coverage, and fail-closed malformed resources

## 2. Entitlement Storage

- [x] 2.1 Add the `nexus-authorization-entitlements` DynamoDB table to the core stack: consumer-domain partition key, entitlement-id sort key, producer reverse GSI (`PRODUCER#{producerDomain}`), audit record item type, an `OLD_IMAGE` stream, and retention policy matching the bindings table
- [x] 2.2 Add entitlement access helpers in `nexus-common` (key construction, get/put/conditional delete, consumer-domain query, producer GSI query, transactional write with audit) with unit tests against the documented key patterns
- [x] 2.3 The event-streams table's base key (`producerDomain` hash, `{schemaName}#{consumerDomain}` sort) already is the producer reverse index the design wants — no new GSI or backfill is needed. Verify revocation candidate queries and outbound subscription views against it, and add the schema-name prefix query helper that prefix-pattern revocations narrow with

## 3. Grant Engine Extension

- [x] 3.1 Extend `DynamoGrantEngine` to evaluate subscribe-family actions against both stores: binding check plus entitlement coverage for create/enable, binding-only for disable/delete and self-subscription, fail-closed on malformed composite resources, with structured token-free deny logging
- [x] 3.2 Extend `InMemoryGrantEngine` with fixture-fed entitlements mirroring the same evaluation rules
- [x] 3.3 Extend the shared decision-parity fixture suite with two-table cases (covering `*` grant, covering exact-name grant, covering prefix grant, overlapping grants, revoked grant, wrong consumer domain, self-subscription) and prove identical decisions on both implementations
- [x] 3.4 Move the subscription command handlers off `IamOnlyAuthorizer` onto the engine (lands with 5.x); retain `IamOnlyAuthorizer` as the standing IAM-only guard for platform-operated control-plane paths (domain registration/update) and update its `nexus-common` doc comments to say so

## 4. Entitlement Administration API

- [x] 4.1 Add strict request/response models for create (consumerDomain, producerDomain, scopePattern), list (consumerDomain or producerDomain filter), and revoke (by entitlementId); tests prove callers cannot supply audit fields or identifiers and that malformed scope patterns are rejected
- [x] 4.2 Add `POST /entitlements`, `GET /entitlements?consumerDomain=|producerDomain=`, and `DELETE /entitlements/{entitlementId}` to the OAuth OpenAPI contract only, wired to the REQUEST authorizer; contract tests prove the routes do not exist on the IAM surface (the existing mixed-auth and VTL contract tests cover every route; the IAM absence proof is added with the section 5 contract tests)
- [x] 4.3 Implement the entitlement administration Lambda: dual authorization path (platform-admin, or publisher binding on the entitlement's producer domain; list additionally allows subscriber binding on the queried consumer domain), idempotent conditional create, conditional revoke, 404/409 mapping, and one `TransactWriteItems` commit per mutation carrying its token-free audit record
- [x] 4.4 Add handler tests for create/list/revoke under each authorization path, non-admin denial, producer self-service scoped to its own domain only, cross-producer denial, idempotent re-create, concurrent-revoke conflict mapping, and audit contents

## 5. Subscription Commands

- [x] 5.1 Reshape both surfaces' write contracts to flat verbs: `POST /subscriptions`, `POST /subscriptions/state`, `DELETE /subscriptions/{subscriptionId}` with `consumerDomain` in request bodies; update direct API Gateway-to-SQS mappings to inject the trusted-principal envelope (OAuth groups or IAM account) while preserving structural validation and `202 Accepted`
- [x] 5.2 Change the subscription command handler to authorize through the engine before any workflow starts: create/enable require subscriber binding plus current covering entitlement; disable/delete require only the binding; domain `ACTIVE` checks, rule-name constraints, and self-subscription semantics are preserved
- [x] 5.3 Perform a final current-entitlement check in the activation path immediately before any EventBridge rule transitions to `ENABLED`
- [x] 5.4 Allow `platform-admin` to operate subscriptions without a subscriber binding while still requiring a current covering entitlement for create/enable
- [x] 5.5 Add command behavior tests on OAuth and IAM paths for authorized create, missing binding, missing entitlement, alternative covering grants, post-revocation disable/delete, denied enable, platform-admin mutation, self-subscription, deterministic failure events, and idempotent retries

## 6. Entitlement Revocation Worker

- [x] 6.1 Add the revocation SQS queue with its own DLQ, redrive policy, and retention matching the established command-queue pattern; add the entitlements-table stream consumer Lambda that translates entitlement `REMOVE` records (old image) into revocation queue messages and `EntitlementRevoked` business events, idempotent on the stream event id, with bisect-batch-on-error, retry caps, and an on-failure destination; consumer tests prove duplicate stream deliveries yield exactly one message and event, and non-entitlement records (audit inserts) are ignored
- [x] 6.2 Implement the revocation worker Lambda: bounded-batch candidate queries through the producer reverse index (narrowed by key equality or prefix where the revoked pattern allows), per-candidate coverage re-evaluation through the engine, idempotent disable of Nexus-managed rules and per-environment desired state in every environment, and retention of rule identities, pinned versions, and subscription records
- [x] 6.3 Add the scheduled reconciliation sweep (EventBridge Scheduler, hourly): a sweep Lambda that scans active subscriptions, re-evaluates each against current coverage through the engine, and disables any uncovered Nexus-managed rules and per-environment desired state, reusing the worker's disable logic
- [x] 6.4 Add worker tests for last-grant revocation, overlapping-grant preservation, revocation during activation, idempotent disablement, redrive with re-evaluation after a new grant, and no mutation of downstream application-bus rules; add sweep tests for drift disablement and no-op when coverage is intact

## 7. Subscription Reads

- [x] 7.1 Implement `GET /subscriptions?domain={domain}&direction=inbound|outbound` on both surfaces: inbound queries the consumer index under a subscriber binding; outbound queries the producer reverse index under a publisher binding with full consumer-topology visibility; support schema, version, and status filters and require direction
- [x] 7.2 Implement canonical `GET /subscriptions/{subscriptionId}` authorization through the visible consumer side, producer side, or platform role, returning `403 Forbidden` for unauthorized callers and `404 Not Found` for missing subscriptions
- [x] 7.3 Add read behavior tests for OAuth and IAM inbound/outbound visibility, publisher full outbound visibility, platform-wide inspection, `platform-auditor` read-only behavior, required direction, filtering, and 403/404 separation

## 8. Subscriber Schema Catalog

- [ ] 8.1 Implement `GET /schemas?consumerDomain={domain}`: read current entitlements, expand scope patterns against per-environment registry listings (exact names looked up directly), deduplicate overlapping coverage, report latest version per environment, and apply producer-domain and environment filters with watermark cursor pagination per the design (sources in producer-domain order, listing resumes at the watermark and stops when the page fills, page size capped at 50, cursor binds the filter set and carries no authorization state)
- [ ] 8.2 Implement `GET /schemas/{schemaName}?consumerDomain={domain}&domain={producerDomain}` with full latest content per environment under the same coverage check; uncovered targets return `403 Forbidden`
- [ ] 8.3 Implement the platform-role bypass: `platform-admin` and `platform-auditor` omit consumerDomain and receive the platform-wide catalog, auditor read-only
- [ ] 8.4 Add catalog behavior tests for grant union expansion, future schemas under a `*` grant, future versions under an exact-name grant, prefix-family expansion including future members with non-matching schemas excluded, empty catalog for zero entitlements, environment skew, stable ordering with no duplicates across pages, revoked coverage absent from later pages, mid-sequence grants excluded behind the watermark, cursor/filter mismatch returning 400, and platform bypass

## 9. Infrastructure and Operations

- [ ] 9.1 Provision the entitlements table and its stream, the event-streams producer GSI, the revocation queue, the stream consumer, the worker, and the sweep schedule, with least-privilege IAM: administration Lambda gets conditional write plus transactional write on entitlements; the stream consumer gets entitlements-stream read, revocation-queue send, and control-plane bus put-events; the worker and sweep get event-streams read/update and EventBridge rule disable only; consumers get none
- [ ] 9.2 Add metrics, structured audit logs, and alarms for authorization denials, revocation queue and DLQ depth, stream-consumer IteratorAge and on-failure destination deliveries, failed rule disablement, reconciliation drift, command outcomes, and catalog latency without logging schema content, tokens, or claims
- [ ] 9.3 Add the seed path for migration: platform-admin tooling that creates subscriber bindings (via the grant-engine admin API) and entitlements for consumer domains retaining access before subscription re-evaluation, and verify rollback never bulk re-enables uncovered rules

## 10. Documentation and Validation

- [ ] 10.1 Update `docs/development/decisions.md` with the two-store model, the scope-pattern grammar (including the prefix naming-hygiene caveat), entitlements as control-plane intent with EventBridge rules as compiled enforcement, composite resource grammar, dual-path entitlement administration, stream-driven side-effect transport (change records versus business events), reconciliation-bounded revocation completion, 403-over-404 denial, and `IamOnlyAuthorizer` retention for platform-operated paths
- [ ] 10.2 Update `docs/development/architecture.md` with the entitlements table, engine composition, revocation worker flow, flat-verb subscription views, and catalog expansion
- [ ] 10.3 Update API and operational documentation with breaking route, request-body, authorization, and visibility migrations
- [ ] 10.4 Run focused crate tests during each red/green/refactor cycle, then `make check` and `make test` for the workspace
- [ ] 10.5 Run strict OpenSpec validation and verify the implementation satisfies every subscriber-entitlements, event-subscriptions, schema-management, async-command-processing, and control-plane-access scenario
