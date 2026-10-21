# Tasks

Implementation only begins after explicit apply authorization. Use red/green/refactor for the lifecycle, shared-library, notification, and API behavior below. Deployment and backfill execution require separate authorization; no live writes are needed to finish these tracked implementation tasks.

## 1. Public contracts and schema governance

- [x] 1.1 Add typed system-event/phase models and canonical draft-4 schemas under `api/system-events/`, including version selectors, compact intermediate range, safe partial reasons, and the eight-or-fewer SNS attributes. Verify serialization fixtures for both triggers and outcomes against the schemas, including null start results, metadata revision 0, and rejection of malformed payloads.
- [x] 1.2 Add deterministic phase identity and generation-derived revision helpers in `konductor-common`. Verify repeat delivery preserves identity/timestamps, start precedes terminal within a generation, newer generations sort higher, and different subscription/environment scopes do not collide.
- [x] 1.3 Implement deployment-time system-schema bootstrap with canonical hash/version comparison and the existing compatibility gate. Verify mocked create, identical retry, compatible append, incompatible/unprovable update, and registry-version drift cases before wiring deployment dependencies.
- [x] 1.4 Add `docs/reference/system-events.md` and a decision record under `docs/decisions/`, index the decision, and link the canonical schemas rather than copying definitions. Verify the reference distinguishes event-contract versions, business versions, metadata revisions, supersession, and at-least-once delivery; add new published pages to `mkdocs.yml`.

## 2. Durable upgrade snapshots in the re-pin lifecycle

- [x] 2.1 Extend `EventStream` storage/serialization with optional top-level per-environment `upgradeEvents` and conditional phase-commit helpers. Verify legacy reads, all write/repair round-trips, duplicate commits, removed/recreated subscriptions, and preservation of unrelated environment facts with focused shared-library tests.
- [x] 2.2 Extend `schema-promotion-steps` and materialization inputs to carry the operation, trigger, generation, each entry's original pin, requested version, metadata selector snapshots, and estimate. Verify promotion and authoring fixtures, different starting pins, metadata activation during draining, and exclusion of failed/awaiting-promotion entries.
- [x] 2.3 Update `materialize-subscription-env/src/repin.rs` to commit start facts and atomically commit terminal pin-or-failure facts, preserving current enablement. Verify complete/partial replay, rule failures, persistence failures, retries after ambiguous commits, and no publication calls in the mutation handler.
- [x] 2.4 Update both re-pin state machines and catch paths to propagate facts, preserve generation guards, and record truthful entry outcomes. Add `SystemEventEmissionEnabled` as a deployment gate through the existing configuration path. Verify workflow/template fixtures for disabled emission, normal drain, superseded work, no eligible entries, and isolation of one failing entry.
- [x] 2.5 Update `docs/architecture/subscriptions.md`, `docs/reference/data-model.md`, and the subscriptions side-effects record with commit boundaries and recovery failure walks. Verify each changed write path answers the durability rubric and no documentation claims completion at registry-publication time.

## 3. Projection, system infrastructure, and failure retention

- [x] 3.1 Add the `system-event-projector` handler over old/new stream images, with typed validation and per-entry `PutEvents` checks. Verify only changed upgrade phases emit, unchanged/REMOVE/legacy records do not, retries reproduce the public payload, and partial batch errors identify the correct retry boundary.
- [x] 3.2 Wire the subscription table stream, projector mapping, least-privilege roles, system bus/archive/registry, bootstrap dependencies, and bounded retry settings in `konductor-core-stack.yaml`. Verify template tests cover `NEW_AND_OLD_IMAGES`, `TRIM_HORIZON`, 90-day archive retention, independence from SES conditions, and emission disabled by default.
- [x] 3.3 Configure full-invocation S3 failure retention for the projector, encrypted/private storage, iterator-age and failure alarms, and distinct EventBridge invocation versus Lambda-processing failure destinations. Verify template/handler fixtures retain an original failed payload beyond the stream window and preserve identity through a mocked targeted redrive.
- [x] 3.4 Add the operational failure-recovery section in `docs/operations/system-events.md` and update the schema/subscription side-effects references and index. Record the naming, historical-schema-read, and SQS-retention discrepancies in the discrepancy register without unrelated behavior changes. Verify the guide distinguishes targeted failed-message recovery from unsupported archive replay and links every alarm to an operator action.

## 4. Topic lifecycle, account policies, and existing-domain backfill

- [x] 4.1 Implement the shared topic ensure/verify/delete helper with managed ownership checks, domain tags, protocol-specific principal/endpoint restrictions, and idempotent SNS operations. Verify owner/override/shared-account policies, deduplication of account IDs, denied foreign principals/endpoints and HTTPS/email/SMS, name collisions, and retry/delete-not-found behavior.
- [x] 4.2 Extend outer domain provisioning and compensation to configure the topic once, persist the ARN only when ready, and clean up owned topics on failure. Verify workflow fixtures cover success, policy/tag failure, all-environment failure, clean rollback, and compensation errors without changing namespace/RAM retention.
- [x] 4.3 Extend cost-center retagging and explicit teardown tooling for recorded domain-level topics. Verify retag skips an absent ARN, disable retains topics/subscriptions, teardown checks ownership, and ordinary per-environment cleanup cannot accidentally remove the shared topic.
- [x] 4.4 Add a dry-run-capable, paginated, per-domain-or-fleet backfill script reusing the topic helper and conditional domain writes. Verify mocked `ACTIVE`/`INACTIVE` selection, page traversal, interruption after topic creation, rerun convergence, per-domain failure reporting, retained endpoint subscriptions, and zero writes in dry-run mode.
- [x] 4.6 Permit email endpoints in the topic subscribe policy alongside SQS/Lambda, keeping HTTPS/SMS/chat denied. Verify the email statement is registered-account-scoped, the unsupported-protocol denial no longer lists email, and the SQS/Lambda endpoint account constraints are unchanged.
- [x] 4.7 Subscribe the domain owner email as the default topic endpoint during provisioning, reconcile it on committed owner-email changes, and ensure it in backfill. Verify idempotent subscribe (no duplicates on retry), pending-confirmation is not an error, old address unsubscribed on owner change, failure observability without metadata rollback, absent-topic skip, and dry-run backfill makes no subscribe calls.
- [x] 4.5 Update provisioning/resource architecture references and the rollout section of the operations guide, including readiness checks, quotas, `SystemEventEmissionEnabled`, and non-destructive rollback. Verify commands against local argument parsing/mocks, document separate authorization for live execution, and classify provisioning/backfill/retag effects in the domain-management side-effects record.

## 5. Recipient routing and consumer setup

- [x] 5.1 Add `system-event-router` to resolve and validate the recorded recipient topic, publish the full event and exact attributes, and surface failures. Verify domain isolation, inactive-domain retained topics, absent/mismatched ARN rejection, SNS transient failures, duplicate delivery, and no destination derived from arbitrary event input.
- [x] 5.2 Wire the single routing rule, Lambda permission, scoped SNS publishing role, and invocation/processing failure handling. Verify template tests prove domain count does not increase routing-rule count and no domain principal gains system-bus publishing or topic-administration rights.
- [x] 5.3 Add `docs/guides/system-events.md` with SQS/Lambda setup, source-topic-scoped endpoint policies, subscription ownership, filters, SNS wrapper/raw delivery, idempotency, current-state initialization, and exact contract fetching. Verify examples with local fixtures, add navigation links, and document router/projector durability without promising archive replay or exactly-once external actions.

## 6. Optional promotion-owner emails

- [x] 6.1 Add promotion consumer-start/outcome and producer-failure notification handling using existing owner lookup, secondary recipients, HTML escaping, and SES classification. Verify both promotion phases, divergence before re-pinning, partial outcomes, absent optional failure detail, no authoring emails, and permanent/transient SES errors with mocked handler tests.
- [x] 6.2 Wire the consumer public-event rule and producer internal-event rule under the existing notification condition, independent of topic delivery. Verify disabled-SES templates still retain system infrastructure; update notification/lifecycle references and side-effects records, explicitly documenting delayed/duplicate/out-of-order email limits.

## 7. Domain discovery API and console reference

- [x] 7.1 Add optional top-level `systemEventsTopicArn` to both OpenAPI detail schemas and `describe-domain`. Verify API/handler tests for recorded versus absent fields, OAuth/IAM parity, unchanged owner/platform visibility, names-only directory exclusion, and no writable topic field.
- [x] 7.2 Update the UI type and domain detail System events card using `NxCard`, `NxButton`, and `useCopyToClipboard()`. Add mocked Playwright coverage for owner/auditor visibility, one domain-level ARN, copy feedback, missing configuration, unavailable detail, narrow viewport wrapping, and absence of topic administration/console/replay controls; verify `rtk pnpm lint` and `rtk pnpm typecheck` from `web/`.
- [x] 7.3 Link console guidance to the consumer guide and update affected API/UI reference links plus `CHANGELOG.md` under Unreleased. Verify wording covers SQS/Lambda-only subscriptions, existing-domain backfill, audit-only retention, and no new schema-read permission; keep endpoint inventories in OpenAPI.
- [x] 7.4 Stamp the latest-event marker (`systemEventsLastEventAt`/`Type`) from the router after a successful publish (newer-wins conditional write, non-fatal on failure), expose it on list and describe domain APIs in both OpenAPI surfaces, and extend api_surfaces. Verify newer/duplicate/out-of-order marker behavior and failure tolerance.
- [x] 7.5 Replace the domain-page System events card with a top-level System events navigation item (dedicated icon, unread dot from browser-local last-seen vs. list markers) and a dedicated page listing visible domains' copyable topic ARNs with setup guidance. Rework the Playwright coverage accordingly.
## 8. Cross-cutting acceptance checks

- [x] 8.1 Add an offline integration fixture spanning per-entry phase commit, stream projection, routing, and contract selectors for promotion, authoring, paused entries, partial replay, entry failure, duplicate delivery, and supersession. Verify that notification failures do not reverse committed pins and that the full failed payload remains recoverable without archive replay.
- [x] 8.3 Re-run `rtk make check`, `rtk make test`, the state-machine/backfill Python suites, strict documentation build, and `rtk openspec validate domain-system-events --strict` after the email-subscription delta (4.6-4.7), obtain a focused review of the touched Rust/ASL/script surfaces, and append the results to the acceptance record.
- [x] 8.2 Run `rtk make check`, `rtk make test`, the full `rtk pnpm test:ui` and `rtk pnpm build` from `web/`, strict documentation build in the documented environment, and `rtk openspec validate domain-system-events --strict`. Obtain the project Rust/UI reviews, resolve findings, and record actual results and any unavailable live validation without treating them as passed.
- [x] 8.4 Re-run the full gate suite after the console/marker delta (7.4-7.5), obtain focused reviews, and append results to the acceptance record.

## Acceptance results (recorded at implementation completion)

- `make check`: PASS (fmt, clippy across the workspace, template surface tests).
- `make test`: PASS — full workspace, 63 suites ok, 0 failures (includes the
  7-scenario journey fixture `system-event-router/tests/journey.rs`).
- Web: `pnpm lint`, `pnpm typecheck`, `pnpm build`: PASS; full
  `pnpm test:ui`: 100/100 PASS (6 new `domain-system-events.spec.ts` tests).
- Docs: `uvx --with mkdocs-material mkdocs build --strict`: PASS (one
  pre-existing nav gap fixed: `operations/schema-promotion.md` added to nav).
- Scripts: `python3 -m unittest` — scripts/tests 27/27, docs-site 9/9 PASS.
- `openspec validate domain-system-events --strict`: PASS.
- Reviews: rust-reviewer returned 6 should-fix findings, all resolved
  (conflict-resolution eligibility ordering + strongly consistent reread,
  module splits `attrs.rs`/`event.rs`, typed `DomainStatus` in the router,
  `deny_unknown_fields` on the topic-manager DTO, loud warning on dropped
  malformed snapshot entries, checked metadata-revision conversion; projector
  now receives the real organization). ui-reviewer: APPROVE, 2 advisory nits
  (hardcoded docs host; CSS attribute selector in one absence assertion).
### Delta: default owner email subscription (2026-10-21, tasks 4.6-4.7, gate 8.3)

- User revision after initial completion: topic policy permits email endpoints
  for registered accounts; provisioning and backfill subscribe the domain
  owner email by default (pending SNS confirmation); committed owner-email
  changes reconcile the subscription via a new
  `DomainOwnerEmailChangeRule` → `system-topic-manager` path
  (`reconcileOwnerEmail`); HTTPS/SMS stay denied.
- Gates after the delta: `make check` PASS, workspace `cargo test` PASS
  (63 suites; konductor-common lib 455 tests), python suites 29/29 + 9/9 PASS,
  UI lint/typecheck PASS and `domain-system-events.spec.ts` 6/6 PASS,
  mkdocs strict PASS, `openspec validate --strict` PASS.
- Focused rust-reviewer pass on the delta: 2 findings, both resolved
  (backfill guard against a missing `ownerEmail`; subscription helpers split
  into `system_events/subscriptions.rs` to stay under module size limits),
  plus pagination/error-classification/handler-boundary test nits folded in.
### Delta: top-level System events console page + unread indicator (2026-10-21, tasks 7.4-7.5, gate 8.4)

- User revision after the email delta: system-events discovery moved from
  the domain-detail card to a top-level console navigation item (dedicated
  bell icon) with a browser-local unread dot; the new `/system-events` page
  lists every visible domain's copyable topic ARN with setup guidance. No
  event feed and no server-side read state: the router stamps a newer-wins
  latest-event marker (`systemEventsLastEventAt`/`systemEventsLastEventType`)
  on the domain record after each successful publish (warn-only on failure),
  list/describe expose it, and the console compares it against a localStorage
  last-seen value per browser.
- Gates after the delta: `make check` PASS, workspace `cargo test` PASS
  (router 9/9 lib + 7/7 journey; list-domains 13/13; describe-domain 16/16;
  api_surfaces 74/74), python suites 29/29 + docs-site 9/9 PASS, UI
  lint/typecheck PASS and full `pnpm test:ui` 106/106 PASS (12 new
  `system-events.spec.ts` tests replacing the 6 card-focused ones), mkdocs
  strict PASS, `openspec validate --strict` PASS.
- Focused reviews on the delta: rust-reviewer returned 2 findings, both
  resolved (panic on non-service marker-write errors — now logs the
  `SdkError` itself; missing side-effects record for the router's first
  DynamoDB write — recorded as STAY in `subscriptions.md` with the
  no-domains-stream rationale). ui-reviewer returned 2 blocking + 4
  needs-fix findings, all resolved (semantic `bg-warning` token on the dot,
  `NxEmptyState` `body` prop, auth middleware, list-page heading size,
  error/empty-state and describe-failure/missing-topic conflation, hoisted
  guide URL constant; empty-state and describe-failure test cases added).
- NOT performed (require explicit authorization, per the workflow follow-up):
  live deployment, domain backfill execution, and real SNS/SES delivery
  verification. Emission stays gated behind `SystemEventEmissionEnabled=false`.

## Workflow follow-up

- Review the artifacts, then explicitly start the apply workflow before implementation.
- Before live rollout, obtain authorization for deployment, domain backfill, and test subscriptions. Follow the staged migration plan and verify topic coverage before enabling emission.
- After implementation verification and review, synchronize all seven capability deltas and archive the change through the OpenSpec archive workflow. No archive or deployment is performed by this planning request.
