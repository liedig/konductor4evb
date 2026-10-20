# Tasks

## 1. Spikes

- [x] 1.1 Spike `UpdateSchema` behavior on a scratch registry: append content identical to the latest version, retry with the same `ClientTokenId`, and compare `DescribeSchema` content with what was written. Record the findings in design.md decision 5 and verify that the replay step's post-write version check covers every observed outcome.
- [x] 1.2 Spike `AWS::LanguageExtensions` + SAM `Fn::ForEach` stage generation on a scratch stack (SAM-managed stage = `environments[0]`, conditional generated stages for the rest), including an update from the current hardcoded stages. Record which approach works (ForEach, or the rendered-fragment fallback) and whether the logical-ID change needs a two-step deploy. Verify with `sam deploy` against `["dev","staging","prod"]` and `["sandbox","qa","live"]`.

## 2. Environment-derived stages and console routing

- [x] 2.1 Generate both APIs' stages from `Environments` per spike 1.2's outcome: a deploy-time rendered `AWS::Include` fragment (long-form intrinsics, default-list logical IDs preserved) for the non-first stages, and a marked-line `StageName` rewrite for the SAM-managed stage when the authoring environment is not `dev`. Remove the explicit `staging`/`prod` stage resources. Verify `sam validate --lint`, that the default render is byte-identical to today's template, and a scratch deploy whose stage list matches a custom environment list.
- [x] 2.2 Replace the `/dev/*`, `/staging/*`, and `/prod/*` CloudFront behaviors with one `/api/*` behavior and a viewer-request CloudFront Function that strips `/api`. Verify on the scratch stack that `/api/{env}/domains` reaches each stage and that SPA deep links still fall back to `index.html`.
- [x] 2.3 Change `make web-dev` and `make deploy-web` to write `apiBaseUrl` as the API root (`/api` or `{webUrl}/api`), update the Nuxt dev proxy, and make `config.ts` reject an `apiBaseUrl` that ends in a stage segment. Verify with a `config.ts` unit check and `pnpm typecheck`.
- [x] 2.4 Add an optional `environment` to `useApi()` (default `environments[0]`) and move the Playwright route mocks and fixtures from `/dev/**` to `/api/**`. Verify that the full `pnpm test:ui` suite passes unchanged.
- [x] 2.5 Confirm that `scripts/validate-config.py`'s drift guard refuses a reordered environment list, and add a test fixture for it. Verify with the script's test run.
- [x] 2.6 Update `docs/reference/configuration.md` (environment order is the promotion order, and stages follow the list), `docs/operations/installation.md` (stage-migration note from 1.2), and `openspec/specs/system-architecture` references in docs. Verify that the links resolve.

## 3. Authoring restricted to the first environment

- [x] 3.1 Write failing `schema-admin` tests: create and append on a non-authoring stage return `400 AuthoringEnvironmentOnly` with no Schemas or grant calls (OAuth, IAM, and `platform-admin` cases). Then implement the guard from the existing `ENVIRONMENTS` global variable in `prepare`. Verify with `rtk cargo test -p schema-admin`.
- [x] 3.2 Document the `400` in both OpenAPI contracts and update `docs/guides/` schema authoring and `docs/reference/api.md` (the guard reads the existing `ENVIRONMENTS` global; no new variable). Verify `sam validate --lint` and that the docs name the authoring environment rule.
- [x] 3.3 Add the breaking-change CHANGELOG entry under `Unreleased`. Verify that the entry names the `400` and the promotion path.
- [x] 3.4 Write failing `schema-admin` tests: a successful create or append writes the version's canonical content and hash to `konductor-schema-versions`; a failed record write after the registry write returns an error; a retried append that hits the registry's no-change conflict completes the missing record, mints no version, and reports the existing version as a duplicate. Then implement the record write and self-heal. Verify with `rtk cargo test -p schema-admin`.

## 4. Grant action and promotion record

- [x] 4.1 Add `schema:promote` to the grant engine, granted by the `publisher` role, with `grant_parity.rs` cases for publisher allow, consumer deny, and platform roles. Verify with `rtk cargo test -p konductor-common`.
- [x] 4.2 Add a `SchemaPromotion` model (phase and reason enums, versions, actor, `drainUntil`, re-pin counts), a `RepinState` model, and a `SchemaVersionContent` model (canonical content, SHA-256 hash, `writtenBy`, `writtenAt`) to `konductor-common`, with serde round-trip tests. Verify with `rtk cargo test -p konductor-common`.
- [x] 4.3 Add the `konductor-schema-promotions` table (`pk`/`sk`, stream `NEW_AND_OLD_IMAGES`, PITR, KMS) and the `konductor-schema-versions` table (`pk = {domain}#{eventName}#{environment}`, `sk` = version number, PITR, KMS, no stream) to the core stack, and document both in `docs/reference/data-model.md`. Verify with `sam validate --lint`.

## 5. Promotion API

- [x] 5.1 Write failing tests for a new `schema-promotions` Lambda's `POST /schemas/promotions`: `202` with an id; `400` on the authoring stage; `403` without `schema:promote`; `404` for an inactive domain or a schema missing from the source; `409` with the active id when a `VALIDATING`/`REPLAYING`/`REPINNING` item exists; acceptance after a terminal phase. Then implement the conditional `PutItem`. Verify with `rtk cargo test -p schema-promotions`.
- [x] 5.2 Write failing tests for `GET /schemas/promotions` (publisher and auditor allowed, others `403`, `404` when absent, full status shape), then implement it with a consistent `GetItem`. Verify with `rtk cargo test -p schema-promotions`.
- [x] 5.3 Add both routes to `api/openapi-oauth.yaml` and `api/openapi-iam.yaml` with request models, and wire the function into the core stack. Verify `sam validate --lint` and an OpenAPI lint pass.
- [x] 5.4 Record the promotion handler's and the updated `schema-admin` write paths' rubric classifications (registry write then content record, with the conflict self-heal) in a new `docs/development/side-effects/schemas.md`, indexed from the side-effects README. Verify that the topic file answers all six rubric questions for each handler.

## 6. Promotion stream consumer and lifecycle events

- [x] 6.1 Write failing tests for `schema-promotion-stream`: a new `PENDING` id starts `promote-{promotionId}` (and treats `ExecutionAlreadyExists` as success); `→REPLAYING`, `→COMPLETED`, and `→FAILED` emit `SchemaPromotionStarted`/`Completed`/`Failed` with the documented payload; `PENDING→FAILED` emits only `Failed`; `REPIN` items are ignored. Then implement. Verify with `rtk cargo test -p schema-promotion-stream`.
- [x] 6.2 Wire the event source mapping (`sk = PROMOTION` filter, bisect, maximum record age, on-failure SQS destination, and an alarm on that destination). Verify `sam validate --lint`.
- [x] 6.3 Document the three lifecycle events in `docs/reference/lifecycle-events.md` and add the consumer's rubric record to `schemas.md`. Verify that the payload fields match the model from 4.2.

## 7. Replay workflow

- [x] 7.1 Write failing tests for the `schema-promotion-steps` `plan` action: in-sync, first arrival, catch-up, `DIVERGED` on a recorded-hash mismatch (reports the first differing version), `DIVERGED` on a target version with no promotion write record (with the latest-version repair path via canonical `DescribeSchema` comparison), and `DIVERGED` when the target is ahead. Then implement it with `ListSchemaVersions` plus versions-table reads. Verify with `rtk cargo test -p schema-promotion-steps`.
- [x] 7.2 Write failing tests for the `replay` action: read content from the versions table; write when the target's latest = N−1 with `ClientTokenId = {promotionId}-v{N}`; treat the registry's no-change conflict and a latest of N as already-written after a canonical comparison (completing the target's record); fail on any other state or a returned version other than N. Then implement. Verify with `rtk cargo test -p schema-promotion-steps`.
- [x] 7.3 Author `event-streams/state-machines/promote-schema.asl.yaml` (Plan, MarkReplaying, Widen, sequential replay Map with `lastWrittenVersion` commits, `events:putEvents` `SchemaVersionPublished` with Retry, MarkDraining, Wait, Narrow, Mark terminal, a top-level Catch to `FAILED`, and a 1-hour timeout) plus the `SchemaRepinDrainSeconds` parameter (default 300, minimum 120). Verify `sam validate --lint` and an ASL validation of the definition.
- [x] 7.4 Confirm that create and append keep their handler-side `SchemaVersionPublished` emission (authoring environment only), and that replay steps emit nothing of their own. Add a `schema-admin` test asserting append emission carries `environment: environments[0]`. Verify with `rtk cargo test -p schema-admin`.

## 8. Re-pinning

- [x] 8.1 Write failing tests for `materialize-subscription-env` `mode: repin`: applies an explicit version list to the same rule name, passes the rule's current `State` from `DescribeRule` (a disabled rule stays disabled), skips targets, alarms, and entitlements, commits `pinnedVersion` only when `commitPin` is set, and records a per-entry `FAILED` on a `PutRule` error. Then implement. Verify with `rtk cargo test -p materialize-subscription-env`.
- [x] 8.2 Change initial materialization and extension to write the `[pinned, pinned + 1]` lookahead pattern, and update the existing pattern assertions. Verify with `rtk cargo test -p materialize-subscription-env -p subscription-extender`.
- [x] 8.3 Write failing tests for the `schema-promotion-steps` `widen` and `narrow` actions on the `REPIN` item: widen takes the min floor and max target and bumps the generation; narrow is conditional on generation and `drainUntil`, exits when superseded, re-applies when the generation moved after apply, and handles partial (`to = lastWritten`) and nothing-written (`to = old pin`) cases. Include the two-quick-appends overlap case. Then implement, with entry enumeration through the `producerDomain` query. Verify with `rtk cargo test -p schema-promotion-steps`.
- [x] 8.4 Author `event-streams/state-machines/repin-schema-env.asl.yaml` (Widen, Wait, Narrow, with a re-pin Map at `MaxConcurrency` 10), and have `subscription-extender` start it as `repin-{domain}-{eventName}-{env}-v{version}` when the event's environment is `environments[0]`. Add extender tests for "authoring starts a re-pin, other environments do not". Verify with `rtk cargo test -p subscription-extender` and `sam validate --lint`.
- [x] 8.5 Update the `materialize-subscription-env` and `subscription-extender` rubric records in `docs/development/side-effects/subscriptions.md`, and update `docs/architecture/subscriptions.md` and `docs/architecture/schemas.md` with the pattern range and re-pin flow. Verify that the pages describe the lookahead and the drain window consistently.

## 9. Console

- [x] 9.1 Move `pages/domains/[name].vue` to `pages/domains/[name]/index.vue` and order every schema environment display (panel, catalog detail) by `config.environments`. Add Playwright cases for configured order with custom names. Verify with `rtk playwright test`.
- [x] 9.2 Change the authoring forms to show `environments[0]` as the fixed target with no selector, and the panel to show compact badges linking to the producer schema page. Update the existing authoring Playwright specs. Verify with `rtk playwright test`.
- [x] 9.3 Build `pages/domains/[name]/schemas/[schema].vue`: the pipeline (versions behind, not-yet-promoted nodes), version history by environment, subscribers by environment with pinned or paused state, the latest promotion per environment, publisher-required and auditor read-only states. Add Playwright coverage per the web-ui scenarios. Verify with `rtk playwright test`, `pnpm lint`, and `pnpm typecheck`.
- [x] 9.4 Add the promote journey: `NxConfirm` listing versions to replay and the re-pin count, the `202` then polled phases (replaying, draining with the expected time, in sync), a `409` toast with progress, `DIVERGED` and `REPLAY_FAILED` explanations, and progress restored after navigation. Add Playwright coverage with exact toast text and focus return. Verify with `rtk playwright test`.
- [x] 9.5 Update `docs/development/design-system.md` if any primitive or interaction contract changed, and run the `ui-reviewer` droid on the console diff. Verify that the review reports no blocking findings.

## 10. Documentation and decisions

- [x] 10.1 Write `docs/decisions/promote-schema-versions.md` (promotion by replay, trust by exact match instead of re-gating, the lookahead, environment-derived stages), mark the `gate-schema-promotion` row in `deferred.md` as superseded with a link, and index the record in `docs/decisions/README.md`. Verify that the dates and supersession links are preserved.
- [x] 10.2 Add a schema promotion guide section to `docs/guides/` (how to promote, what subscribers see, partial and diverged outcomes) and a runbook in `docs/operations/` (publish DLQ redrive before promoting, a stuck `PENDING` lease and redrive from the stream consumer's on-failure queue, a lost re-pin). Verify that the guide links to the API reference instead of copying tables.
- [x] 10.3 Add the CHANGELOG entries for promotion, the console page, and environment-derived stages under `Unreleased`. Verify that each entry has a user, operator, or contributor angle.

## 11. Integration verification

- [x] 11.1 Run `make check` and `rtk cargo test --workspace`, and run the `rust-reviewer` droid on the backend diff. Verify that everything is clean and the review has no blocking findings.
- [x] 11.2 On a deployed dev installation with a subscriber in each environment: append v2 and v3 in dev while publishing continuously, promote to staging and then prod, and confirm zero dropped events (consumer bus counts), aligned version content across registries, final pins at v3, the lifecycle event sequence, and that a deliberately diverged target fails with `DIVERGED` and writes nothing. Record the results in the change's verification notes.
- [x] 11.3 Gate the promotion workflow on pipeline adjacency: `plan` refuses a source that is not the target's immediate predecessor (`SKIPPED_ENVIRONMENT`, no registry reads), covering executions started out-of-band with a planted lease. Add the failure reason to both OpenAPI surfaces, the console copy, and the runbook; cover the gate with unit tests and the copy with a Playwright spec.
- [x] 11.4 Let pre-recording schemas promote: replay and prefix verification fall back to the source registry's immutable per-version content when no content record exists (live-found defect: `REPLAY_FAILED` "source record missing"). Correct design.md's spike-1.1 claim that per-version content is unreadable. Verify with `cargo test -p schema-promotion-steps`.

## Workflow follow-up

- Run `/opsx-verify` against these artifacts, then archive the change after review.
- Start `/opsx-propose` for `domain-system-events`, which consumes the lifecycle events defined here.
