# Tasks: add-event-subscriptions

## 1. Shared Library (lib/nexus-common)

- [ ] 1.1 Add event-envelope module: derive `source` from namespace identity, compose/split `{source}@{detail-type}` schema names, build rule patterns (source + detail-type + numeric `detail.metadata.schemaVersion`)
- [ ] 1.2 Add rule-name derivation `{consumerDomain}-{detailType}` with validation (charset `[A-Za-z0-9._-]`, ≤ 64 chars) and unit tests covering boundary cases
- [ ] 1.3 Add event-stream models: stream record with per-env map (`pinnedVersion`, `ruleName`, `ruleArn`, `enabled`, `status`), status enum (`PENDING|ACTIVE|FAILED|REMOVED`), subscription-id compose/parse (`{consumer}.{producer}.{detailType}`, split on first two dots)
- [ ] 1.4 Add DynamoDB helpers for the event-streams table: put/get/update, producer+schema-prefix query (version census), consumer GSI query, TTL on removal

## 2. API Contract (api/openapi.yaml)

- [ ] 2.1 Add subscription write endpoints: `POST /subscriptions`, `PATCH /subscriptions/{subscriptionId}`, `DELETE /subscriptions/{subscriptionId}` — SQS integrations with VTL mapping (path params + caller account identity injected), structural validation schemas, 202 responses; no version parameter on subscribe
- [ ] 2.2 Add read endpoints: `GET /subscriptions` (filters: consumer, producer, schema, version, status), `GET /subscriptions/{subscriptionId}`, `GET /schemas?domain=`, `GET /schemas/{schemaName}?domain=` — `aws_proxy` integrations
- [ ] 2.3 Validate the OpenAPI document and regenerate any derived artifacts

## 3. Infrastructure (nexus-core-stack.yaml)

- [ ] 3.1 Add `nexus-event-streams` DynamoDB table: PK `producerDomain`, SK `{schemaName}#{consumerDomain}`, `by-consumer` GSI, TTL on `expiresAt`
- [ ] 3.2 Add subscriptions command queue + DLQ (≥ 14 days retention), wire API Gateway SQS integration role
- [ ] 3.3 Add IAM: subscription command handler role, workflow role (EventBridge PutRule/PutTargets/RemoveTargets/DeleteRule/Enable/DisableRule on `{org}-*-event-bus`, `schemas:DescribeSchema/ListSchemas`, streams table access), delivery role used by rule targets for cross-account PutEvents
- [ ] 3.4 Run cfn-lint and deploy to a dev stack to verify template validity

## 4. Provisioning Delta + Backfill

- [ ] 4.1 Add the platform-delivery resource-policy step to `provision-env.asl.yaml` (after app bus creation, via trust role `events:PutPermission`), extend compensation to cover it
- [ ] 4.2 Verify `deprovision-env.asl.yaml` needs no change (policy dies with the bus) and document that in the workflow comment
- [ ] 4.3 Create idempotent backfill (script or ops workflow + Makefile target): for each ACTIVE domain × env, assume trust role, attach delivery policy; safe to re-run
- [ ] 4.4 Test provisioning end-to-end in dev: new domain's app buses carry the delivery policy; backfill run on a pre-change domain attaches it

## 5. Subscription Workflows (state-machines)

- [ ] 5.1 Author `activate-subscription.asl.yaml`: Map over environments — resolve latest schema version (`schemas:DescribeSchema`), `PutRule` (pattern + state from desired enablement), `PutTargets` (consumer app bus, `DeadLetterConfig` → producer subscription DLQ, retry policy); saga compensation deletes created rules; all-or-nothing failure with per-env error detail
- [ ] 5.2 Author `remove-subscription.asl.yaml`: per env `RemoveTargets` → `DeleteRule`, idempotent ("already gone" = no-op); mark record `REMOVED`, set `removedAt` + `expiresAt` TTL
- [ ] 5.3 Emit lifecycle events from workflows: `SubscriptionActivated`, `SubscriptionActivationFailed` (failed env/step, compensation errors), `SubscriptionRemoved`
- [ ] 5.4 Wire workflow state transitions to the streams table (`PENDING` → `ACTIVE|FAILED`, per-env materialized status)

## 6. Subscription Command Handler (functions)

- [ ] 6.1 Scaffold subscription command handler crate consuming the subscriptions queue, dispatching by `HttpMethod` (POST → subscribe, PATCH → update state, DELETE → unsubscribe)
- [ ] 6.2 Implement subscribe: validate domains ACTIVE, caller account == consumer `ownerAccountId`, rule-name constraints; idempotent on existing ACTIVE subscription; fresh contract on re-subscribe after removal; write `PENDING` record; start activation workflow
- [ ] 6.3 Implement unsubscribe: ownership validation, start removal workflow
- [ ] 6.4 Implement per-env enable/disable (PATCH): update desired state, apply `EnableRule`/`DisableRule` per env, update record
- [ ] 6.5 Emit `{Operation}Succeeded|Failed` outcome events; deterministic validation failures emit `Failed` and acknowledge (no retry/DLQ)
- [ ] 6.6 Unit tests per AGENTS.md conventions covering dispatch, validation matrix (ownership, inactive domain, rule-name budget, self-subscription allowed), idempotency

## 7. Read Functions

- [ ] 7.1 `list-subscriptions` Lambda: filters (consumer via GSI, producer/schema/version via base table), status filter, pagination
- [ ] 7.2 `describe-subscription` Lambda: parse subscription id, GetItem, full per-env detail
- [ ] 7.3 `list-schemas` / `describe-schema` Lambdas: merge `ListSchemas`/`DescribeSchema` across env registries keyed by schema name, latest version per env, 404 for missing/inactive domain or unknown schema
- [ ] 7.4 Unit tests for read functions (filter combinations, id parsing with dotted event names, env merge with version skew)

## 8. Observability

- [ ] 8.1 Add subscription metrics to the per-domain dashboard (rule matches, delivery failures, DLQ depth already present) for producer-side visibility
- [ ] 8.2 Resolve the zero-events alarm placement question (per-subscription vs. per-bus; design leans per-subscription) and implement the chosen default
- [ ] 8.3 Alarm on subscriptions command DLQ depth

## 9. Integration Verification

- [ ] 9.1 End-to-end smoke test in dev (extend Makefile smoke target): register two domains → register a schema in producer registry → subscribe → publish conforming event to producer platform bus → assert delivery on consumer app bus → disable prod-equivalent env and assert stop → unsubscribe → assert rules removed and record `REMOVED`
- [ ] 9.2 Negative-path checks: subscribe to missing schema (all-or-nothing failure + compensation), mis-stamped event (string version → no delivery), duplicate subscribe (idempotent)
- [ ] 9.3 Verify version census query returns per-env pinned versions including a deliberately skewed registry (dev ahead of prod)

## 10. Documentation

- [ ] 10.1 Update `docs/development/architecture.md`: subscription flow diagram, event-streams table, envelope contract, delivery-policy trust change, new workflows (required — architecture is affected)
- [ ] 10.2 Update `docs/development/decisions.md`: record D1–D9 (domain-only subscribers, latest-pinned versions, resource-policy delivery replacing b2b roles, rule naming, envelope, streams table, per-env enable/disable, async write path, read-only discovery)
- [ ] 10.3 Update README: API table (subscription + schema endpoints), project structure, resource topology additions
- [ ] 10.4 Update `docs/domain-management.md`: consumer guide (discover schemas, subscribe, enable/disable, unsubscribe, orphan-rule responsibility) and producer guide (envelope stamping obligation until Publish API exists)
- [ ] 10.5 Update `trust-stack-deployment.md` only if messaging about the unused b2b policy is warranted (no functional change)
