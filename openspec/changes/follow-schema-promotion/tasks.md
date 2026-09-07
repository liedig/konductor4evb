# Tasks: follow-schema-promotion

## 1. Stream model and read surface

- [x] 1.1 Test: `EventStream` round-trips a per-env entry with status `AWAITING_PROMOTION` (null `ruleArn`/`pinnedVersion`) and an optional per-env `errorDetail`. Implement the `StreamStatus::AwaitingPromotion` variant and the `EnvStream.errorDetail` field in `konductor-common/src/streams.rs`; verify the new test passes
- [x] 1.2 Test: `list-subscriptions` summaries and subscription detail include top-level and per-env `errorDetail` when present and omit it when absent. Implement in the read handlers; verify the new handler tests pass and the existing read tests stay green

## 2. materialize-subscription-env Lambda

- [x] 2.1 Create the `materialize-subscription-env` function crate under `event-streams/functions/` with input `{producerDomain, consumerDomain, consumerAccountId, source, detailType, schemaName, ruleName, environment, enabled}`; verify `cargo check -p materialize-subscription-env` passes
- [x] 2.2 Test: happy path — describe resolves latest version, `PutRule` upserts the version-pinned pattern with state from `enabled`, `PutTargets` sets the consumer app bus with the producer DLQ, the zero-events alarm is created, and the record update merges only its own env key (`SET environments.#env = ...`). Implement; verify the smithy-mocks test passes
- [x] 2.3 Test: schema absent in the environment (`ResourceNotFound` from describe) records the env entry `AWAITING_PROMOTION` with no rule created and no error raised. Implement; verify
- [x] 2.4 Test: final coverage re-check via the konductor-common grant engine `covers()` — revoked entitlement (enabled env, non-self subscription) records the env `FAILED` with the denial and creates no rule; disabled envs and self-subscriptions skip the check. Implement; verify
- [x] 2.5 Test: re-invoking for an already-materialized env is a no-op beyond upsert idempotency (no duplicate target, record unchanged in shape). Implement any needed conditionals; verify

## 3. Activation workflow rewrite (activate-subscription.asl.yaml)

- [x] 3.1 Replace the inline per-env states (describe, JSONata coverage query, PutRule/PutTargets/alarm, rollback chain) with a single `materialize-subscription-env` Lambda invocation per Map item; delete `RollbackCreatedRules` and its helpers; verify the ASL parses (`sam validate` or equivalent YAML/ASL lint)
- [x] 3.2 Post-Map aggregation: top-level `ACTIVE` when ≥1 env materialized, else `FAILED` with `errorDetail` naming schema/producer; top-level record update sets only `status`/`errorDetail`/`updatedAt` (never replaces the `environments` map); `SubscriptionActivated` / `SubscriptionActivationFailed` carry per-env outcomes including awaiting envs. Verify via workflow-level tests or a local Step Functions/simulated run if the repo supports it; otherwise verify by inspection plus the e2e task 8.2

## 4. update_state desired-state reconciliation

- [x] 4.1 Test: pausing or resuming an `AWAITING_PROMOTION` env records the desired flag without any EventBridge call (no rule exists to flip). Implement; verify
- [x] 4.2 Test: setting `enabled: true` on a per-env `FAILED` entry invokes `materialize-subscription-env` for that env (after the existing entitlement re-check) and the entry becomes `ACTIVE` on success. Implement; verify
- [x] 4.3 Verify existing update_state tests (materialized pause/resume kill switch) stay green

## 5. schema-admin promotion signal

- [ ] 5.1 Test: successful `POST /schemas` emits `SchemaVersionPublished` on the control-plane bus with `{producerDomain, schemaName, environment, version: 1, publishedAt}` (source `konductor.schema-management`); same for append with the new version. Implement emission in `schema-admin`; verify
- [ ] 5.2 Test: when the registry write succeeded but emission fails, the API response still reports success and the failure is logged. Implement; verify
- [ ] 5.3 Record the handler-emission side-effect exception (registry write has no DDB stream) in `docs/development/decisions.md`; verify the entry exists and follows the rubric format

## 6. Subscription extender

- [ ] 6.1 Test: a `SchemaVersionPublished` event for env E extends every `ACTIVE` subscription on that (producer, schema) whose record has E absent or `AWAITING_PROMOTION`, invoking materialization and emitting `SubscriptionEnvironmentExtended` with identity, env, pinned version, enabled. Implement the extender Lambda; verify
- [ ] 6.2 Test: extender skips `PENDING`/`FAILED`/`REMOVED` streams, no-ops on already-materialized envs, and honors a recorded desired-disabled flag (rule created `DISABLED`). Implement; verify
- [ ] 6.3 IaC: add the extender function, its control-plane bus rule on `SchemaVersionPublished` with a DLQ, and IAM (streams query/update, schemas describe, events PutRule/PutTargets, CloudWatch alarm, Lambda invoke, PutEvents) to the event-streams template; verify `sam validate` (or `cfn-lint`) passes

## 7. Unsubscribe hardening (orphan-rule race)

- [ ] 7.1 Test: unsubscribe deletes the deterministic rule name (`{consumerDomain}-{detailType}`) on every producer env bus even when the record has no materialized entry for that env, treating not-found as a no-op. Implement; verify the test passes and existing unsubscribe tests stay green

## 8. Web UI and end-to-end

- [ ] 8.1 Web types and views: `errorDetail` (top-level and per-env) and the `AWAITING_PROMOTION` status rendered on "My subscriptions" and subscription detail; pause/resume offered for awaiting envs; catalog subscribe journey states which envs materialize now vs follow on promotion. Verify `pnpm lint`, `pnpm typecheck`, and `pnpm build` pass
- [ ] 8.2 E2E on the live dev deployment: re-subscribe shipping to `acme-billing@OrderNotification` — expect dev `ACTIVE`, staging/prod `AWAITING_PROMOTION`, stream `ACTIVE`; then register the schema in the staging registry via the API and verify the subscription extends automatically (`SubscriptionEnvironmentExtended`, staging rule live); finally unsubscribe and verify all env rules including staging are gone. Verify via console/CLI reads of the stream record and buses
- [ ] 8.3 Update `docs/development/architecture.md` where it describes the activation saga/all-or-nothing contract; verify the description matches the new per-env behavior
