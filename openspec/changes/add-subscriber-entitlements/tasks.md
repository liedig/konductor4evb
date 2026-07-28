## 1. Prerequisites and Authorization Model

- [ ] 1.1 Confirm `add-cognito-access-foundation`, `evaluate-authorization-engine`, and `add-publisher-apis` are implemented, and wire this capability to their trusted-principal, role-decision, publisher-binding, and schema-operation contracts.
- [ ] 1.2 Add shared typed models and validation for Cognito-team-group and AWS-account role-binding principals, producer-domain grants, exact producer-schema grants, current/revoked state, and authorization actions.
- [ ] 1.3 Implement the common authorization adapter that combines OAuth caller-group or IAM AWS-account bindings with the union of current consumer-domain grants and supports `platform-admin`, `platform-auditor`, subscriber, and publisher decisions.
- [ ] 1.4 Add behavior tests for group and AWS-account domain isolation, IAM account-granularity across roles/sessions, equivalent OAuth/IAM decisions, exact versus producer-domain coverage, cross-environment logical grants, overlapping grants, global inspection, and `platform-auditor` mutation denial.

## 2. Entitlement Administration and Persistence

- [ ] 2.1 Add persistence access paths for bindings and entitlements by consumer domain, producer domain, exact schema, and revocation state using the DynamoDB-backed grant engine.
- [ ] 2.2 Implement platform-admin-only create and revoke operations plus platform-admin and platform-auditor read-only inspection for subscriber bindings and producer entitlements through the access foundation's administration surface.
- [ ] 2.3 Record auditable actor, target, grant type, scope, and timestamp data for entitlement and binding changes without exposing protected details to unauthorized callers.
- [ ] 2.4 Add integration tests proving OAuth platform administrators can manage authorization state, `platform-auditor` is read-only, IAM has no global-role bypass, and revoked grants are excluded from new decisions.

## 3. Subscription API and Command Authorization

- [ ] 3.1 Update both OAuth and IAM OpenAPI surfaces with domain-scoped create, list, patch, and delete routes, retain canonical subscription detail, require list direction, remove the global list route, reject body-level `consumerDomain`, and use only `subscriptions` terminology.
- [ ] 3.2 Update both surfaces' direct API Gateway-to-SQS mappings to inject consumer-domain and subscription-id path parameters plus the common trusted OAuth or IAM principal envelope while preserving structural validation and `202 Accepted` responses.
- [ ] 3.3 Change subscription ownership to consumer domain and replace account-ownership validation with current subscriber-binding and entitlement checks in the command handler.
- [ ] 3.4 Enforce current entitlement for create and enable while allowing an authorized subscriber to disable or delete after entitlement revocation and allowing `platform-admin` to operate subscriptions without a subscriber binding.
- [ ] 3.5 Add command behavior tests on OAuth group and IAM AWS-account paths for authorized create, missing role, missing grant, alternative covering grants, path/body mismatch prevention, post-revocation disable/delete, denied enable, `platform-admin` mutation with current entitlement, deterministic failure events, and idempotent retries.

## 4. Entitlement Revocation

- [ ] 4.1 Implement a durable revocation workflow that queries affected active subscriptions, re-evaluates each producer schema against all current grants, and tracks per-subscription completion.
- [ ] 4.2 Disable all Nexus-managed EventBridge rules and per-environment desired state when no current grant covers a subscription, while retaining the subscription record, rule identity, and pinned versions.
- [ ] 4.3 Add a final current-entitlement check before activation or enablement changes an EventBridge rule to `ENABLED`.
- [ ] 4.4 Prevent revocation completion until every affected subscription is covered, disabled, or represented by a durable retry/failure record; use the production completion bound recommended by the authorization-engine spike.
- [ ] 4.5 Add integration tests for last-grant revocation, overlapping-grant preservation, revocation during activation, partial disable failure, durable failure recording, resumed retries with coverage re-evaluation, idempotent rule disablement, and no mutation of downstream application-bus rules.

## 5. Subscription Reads

- [ ] 5.1 Implement shared inbound consumer-index and outbound producer queries on both OAuth and IAM surfaces, authorizing Cognito team groups or AWS accounts for subscriber and publisher access and supporting schema, version, and status filters.
- [ ] 5.2 Implement canonical detail authorization through the visible consumer side, producer side, or global platform role and return indistinguishable `404 Not Found` responses for missing or unauthorized resources.
- [ ] 5.3 Add read behavior tests for OAuth and IAM subscriber inbound visibility, publisher full outbound visibility, OAuth platform-wide inspection, `platform-auditor` read-only behavior, required direction, filtering, and detail concealment.

## 6. Subscriber Schema Catalog

- [ ] 6.1 Preserve the publisher-authorized schema creation and append-only version routes from `add-publisher-apis` while replacing only global discovery with the consumer catalog.
- [ ] 6.2 Implement producer-domain grant expansion and exact-schema lookup against the per-environment schema registries, deduplicating overlapping grants without introducing a catalog table.
- [ ] 6.3 Implement catalog list metadata with latest versions per environment, producer-domain and environment filters, and opaque pagination on both OAuth and IAM surfaces.
- [ ] 6.4 Implement catalog detail with full latest schema content per environment and common OAuth-group or IAM-account subscriber-plus-entitlement authorization.
- [ ] 6.5 Add catalog behavior tests for preserved publisher writes, equivalent OAuth/IAM access, grant union expansion, future schemas under a producer-domain grant, exact-schema isolation, environment skew, pagination, revoked access, and indistinguishable detail `404` responses.

## 7. Infrastructure and Operations

- [ ] 7.1 Provision authorization and revocation workflow resources, indexes, IAM permissions, retry behavior, and observability needed by the DynamoDB-backed grant engine and existing subscription stack.
- [ ] 7.2 Add metrics, structured audit logs, and alarms for authorization denials, revocation progress, durable partial failures and retries, failed rule disablement, command outcomes, and catalog latency without logging schema content or sensitive claims.
- [ ] 7.3 Add deployment migration support to seed required subscriber bindings and entitlements before re-evaluating existing active subscriptions, and verify rollback never bulk re-enables unauthorized rules.

## 8. Documentation and Validation

- [ ] 8.1 Update `docs/development/decisions.md` with consumer-domain subscription ownership, additive entitlement evaluation, domain-scoped routes, and revocation semantics.
- [ ] 8.2 Update `docs/development/architecture.md` with access-control boundaries, command authorization timing, revocation flow, subscription views, and catalog expansion.
- [ ] 8.3 Update affected API and operational documentation with breaking route, request-body, authorization, and `404` concealment migrations.
- [ ] 8.4 Run focused unit and integration tests, then run the full repository `make check` validator and resolve all failures.
- [ ] 8.5 Run strict OpenSpec validation and verify the implementation satisfies every subscriber-entitlements, event-subscriptions, schema-management, and async-command-processing scenario.
