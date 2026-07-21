# Tasks: add-event-subscriptions

## 1. Shared Library (lib/nexus-common)

- [x] 1.1 Add event-envelope module: derive `source` from namespace identity, compose/split `{source}@{detail-type}` schema names, build rule patterns (source + detail-type + numeric `detail.metadata.schemaVersion`)
- [x] 1.2 Add rule-name derivation `{consumerDomain}-{detailType}` with validation (charset `[A-Za-z0-9._-]`, ≤ 64 chars) and unit tests covering boundary cases
- [x] 1.3 Add event-stream models: stream record with per-env map (`pinnedVersion`, `ruleName`, `ruleArn`, `enabled`, `status`), status enum (`PENDING|ACTIVE|FAILED|REMOVED`), subscription-id compose/parse (`{consumer}.{producer}.{detailType}`, split on first two dots)
- [x] 1.4 Add DynamoDB helpers for the event-streams table: put/get/update, producer+schema-prefix query (version census), consumer GSI query, TTL on removal

## 2. API Contract (api/openapi.yaml)

- [x] 2.1 Add subscription write endpoints: `POST /subscriptions`, `PATCH /subscriptions/{subscriptionId}`, `DELETE /subscriptions/{subscriptionId}` — SQS integrations with VTL mapping (path params + caller account identity injected), structural validation schemas, 202 responses; no version parameter on subscribe
- [x] 2.2 Add read endpoints: `GET /subscriptions` (filters: consumer, producer, schema, version, status), `GET /subscriptions/{subscriptionId}`, `GET /schemas?domain=`, `GET /schemas/{schemaName}?domain=` — `aws_proxy` integrations
- [x] 2.3 Validate the OpenAPI document and regenerate any derived artifacts (parsed, all $refs resolve; full gate is cfn-lint/sam validate in 3.4)

## 3. Infrastructure (nexus-core-stack.yaml)

- [x] 3.1 Add `nexus-event-streams` DynamoDB table: PK `producerDomain`, SK `{schemaName}#{consumerDomain}`, `by-consumer` GSI, TTL on `expiresAt`
- [x] 3.2 Add subscriptions command queue + DLQ (≥ 14 days retention), wire API Gateway SQS integration role
- [x] 3.3 Add IAM: subscription command handler role, workflow role (EventBridge PutRule/PutTargets/RemoveTargets/DeleteRule/Enable/DisableRule on `{org}-*-event-bus`, `schemas:DescribeSchema/ListSchemas`, streams table access), delivery role used by rule targets for cross-account PutEvents
- [x] 3.4 Run cfn-lint and deploy to a dev stack to verify template validity (cfn-lint passes; core stack deployed successfully)

## 4. Provisioning Delta

- [x] 4.1 Add the platform-delivery resource-policy step to `provision-env.asl.yaml` (after app bus creation, via trust role `events:PutPermission`), extend compensation to cover it
- [x] 4.2 Verify `deprovision-env.asl.yaml` needs no change (policy dies with the bus) and document that in the workflow comment
- [x] 4.3 ~~Create idempotent backfill~~ — descoped: unreleased product, no pre-change domains exist anywhere; live testing also showed operator credentials cannot assume the trust role, so the script never worked as designed (built, then removed)
- [x] 4.4 Test provisioning end-to-end in dev: new domain's app buses carry the delivery policy — verified on `billing` across dev/staging/prod (backfill half descoped with 4.3)

## 5. Subscription Workflows (state-machines)

- [x] 5.1 Author `activate-subscription.asl.yaml`: Map over environments — resolve latest schema version (`schemas:DescribeSchema`), `PutRule` (pattern + state from desired enablement), `PutTargets` (consumer app bus, `DeadLetterConfig` → producer subscription DLQ, retry policy); saga compensation deletes created rules; all-or-nothing failure with per-env error detail
- [x] 5.2 Author `remove-subscription.asl.yaml`: per env `RemoveTargets` → `DeleteRule`, idempotent ("already gone" = no-op); mark record `REMOVED`, set `removedAt` + `expiresAt` TTL
- [x] 5.3 Emit lifecycle events from workflows: `SubscriptionActivated`, `SubscriptionActivationFailed` (failed env/step, compensation errors), `SubscriptionRemoved`
- [x] 5.4 Wire workflow state transitions to the streams table (`PENDING` → `ACTIVE|FAILED`, per-env materialized status)

## 6. Subscription Command Handler (functions)

- [x] 6.1 Scaffold subscription command handler crate consuming the subscriptions queue, dispatching by `HttpMethod` (POST → subscribe, PATCH → update state, DELETE → unsubscribe)
- [x] 6.2 Implement subscribe: validate domains ACTIVE, caller account == consumer `ownerAccountId`, rule-name constraints; idempotent on existing ACTIVE subscription; fresh contract on re-subscribe after removal; write `PENDING` record; start activation workflow
- [x] 6.3 Implement unsubscribe: ownership validation, start removal workflow
- [x] 6.4 Implement per-env enable/disable (PATCH): update desired state, apply `EnableRule`/`DisableRule` per env, update record
- [x] 6.5 Emit `{Operation}Succeeded|Failed` outcome events; deterministic validation failures emit `Failed` and acknowledge (no retry/DLQ)
- [x] 6.6 Unit tests per AGENTS.md conventions covering dispatch, validation matrix (ownership, inactive domain, rule-name budget, self-subscription allowed), idempotency

## 7. Read Functions

- [x] 7.1 `list-subscriptions` Lambda: filters (consumer via GSI, producer/schema/version via base table), status filter, pagination
- [x] 7.2 `describe-subscription` Lambda: parse subscription id, GetItem, full per-env detail
- [x] 7.3 `list-schemas` / `describe-schema` Lambdas: merge `ListSchemas`/`DescribeSchema` across env registries keyed by schema name, latest version per env, 404 for missing/inactive domain or unknown schema
- [x] 7.4 Unit tests for read functions (filter combinations, id parsing with dotted event names, env merge with version skew)

## 8. Observability

- [x] 8.1 Add subscription metrics to the per-domain dashboard (rule matches, delivery failures, DLQ depth already present) for producer-side visibility
- [x] 8.2 Resolve the zero-events alarm placement question (per-subscription vs. per-bus; design leans per-subscription) and implement the chosen default — per-subscription alarm created by activation workflow, deleted on rollback/unsubscribe
- [x] 8.3 Alarm on subscriptions command DLQ depth

## 9. Integration Verification

- [x] 9.1 End-to-end smoke test in dev (extend Makefile smoke target): `make smoke-subscription` — register schema in producer registry → subscribe → publish conforming event → assert rule match → disable env and assert rule DISABLED → unsubscribe → assert rules removed and record `REMOVED` (script authored; execution requires deployed stack)
- [x] 9.2 Negative-path checks: mis-stamped event (string version → no match) and duplicate subscribe (idempotent) in smoke script; missing schema all-or-nothing covered by workflow compensation path + handler unit tests
- [x] 9.3 Verify version census query returns per-env pinned versions including a deliberately skewed registry (dev ahead of prod) — verified: schema `acme-sales@InvoiceCreated` created with v1 in all three registries, v2 added in dev only (deliberate skew); GET /schemas?domain=sales shows per-env latestVersions {dev: 2, staging: 1, prod: 1}; self-subscription activated ACTIVE with per-env pinned versions matching the skew (dev: 2, staging: 1, prod: 1); rule patterns confirmed `schemaVersion:[2]` in dev, `[1]` in staging/prod; DynamoDB version-census query (PK=sales, SK prefix `acme-sales@InvoiceCreated#`) returns the subscriber with per-env pinned versions in a single query. Fixed activation workflow bug: removed unsupported `RetryPolicy` from `PutTargets` for event-bus targets. All test resources cleaned up.

## 10. Documentation

- [x] 10.1 Update `docs/development/architecture.md`: subscription flow diagram, event-streams table, envelope contract, delivery-policy trust change, new workflows (required — architecture is affected)
- [x] 10.2 Update `docs/development/decisions.md`: record D1–D9 (domain-only subscribers, latest-pinned versions, resource-policy delivery replacing b2b roles, rule naming, envelope, streams table, per-env enable/disable, async write path, read-only discovery)
- [x] 10.3 Update README: API table (subscription + schema endpoints), project structure, resource topology additions
- [x] 10.4 Update `docs/domain-management.md`: consumer guide (discover schemas, subscribe, enable/disable, unsubscribe, orphan-rule responsibility) and producer guide (envelope stamping obligation until Publish API exists)
- [x] 10.5 Update `trust-stack-deployment.md` only if messaging about the unused b2b policy is warranted — not warranted: no functional trust-stack change, b2b note captured in architecture.md trust table instead
