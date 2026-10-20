# Changelog

Notable repository changes, with links to their rationale and limits.
Dates below are merge or commit dates, not release or deployment dates.
This log starts with recent changes; it is not a reconstructed release history.
Older rationale is available through the [decision index](docs/decisions/README.md).

## Unreleased

- Add explicit cross-environment schema promotion: `POST /schemas/promotions`
  on a target stage commits a promotion head item (idempotent, `409` while one
  is active), a promotions-table stream consumer starts the
  `PromoteSchemaWorkflow` state machine (idempotent execution name), and the
  workflow replays the missing versions into the target registry under the
  same version numbers, validating the shared prefix first (`DIVERGED` stops
  without touching the target; `REPLAY_FAILED` resumes where it stopped).
  The workflow also refuses a source that is not the target's immediate
  predecessor (`SKIPPED_ENVIRONMENT`), gating executions started out-of-band.
  Completion re-pins the target environment's subscription rules through a
  widen, drain (`SchemaRepinDrainSeconds`, default 300s), narrow sequence that
  keeps delivery continuous, and `SchemaPromotion{Started,Completed,Failed}`
  lifecycle events land on the platform bus. Status is readable via
  `GET /schemas/promotions`. Authorization uses the new `schema:promote`
  grant action on the producer domain.
  [Decision](docs/decisions/promote-schema-versions.md),
  [runbook](docs/operations/schema-promotion.md).
- API stages are now derived from the configured environment list at
  deploy time: the OAuth API renders one stage per environment (generated
  fragment, drift-guarded by `make check-stages`), CloudFront routes
  `/api/*` to the API with the stage segment preserved, and the console
  resolves requests as `{apiBaseUrl}/{environment}`, rejecting unknown
  stage segments. A stage-less `/api` request still hits the default stage.
- The console gains a producer schema page per (domain, schema): a promotion
  pipeline in configured environment order with behind counts, per-version
  environment reach, subscriber pins, and the promote journey (confirmation
  with replay scope and re-pin count, polled phase progress that survives
  navigation, conflict and failure explanations). Schema management entry
  points moved from the domain page to this page; the domain page keeps
  Register schema only until the first schema exists. Environment displays
  everywhere now follow the configured order instead of API payload order.
- **Breaking:** schema creation and version appends are now accepted only on
  the authoring stage (the first configured environment, `dev` by default).
  Other stages answer `400 AuthoringEnvironmentOnly`, naming the authoring
  environment; later environments receive schema versions only through
  promotion (`POST /schemas/promotions`, added in the same change).
- The repository moved to the `liedig` GitHub organization
  (`https://github.com/liedig/konductor4evb`). Old URLs redirect; update
  local remotes with `git remote set-url origin git@github.com:liedig/konductor4evb.git`.
- Publish the documentation as a static site (MkDocs Material) built from
  `docs/`, with a strict link-integrity gate on pull requests and GitHub
  Pages deployment from `main`. Links that point outside the docs tree
  resolve to GitHub file views at build time; source documents are
  unchanged. The strategy documents stay in the repository but off the
  site. Navigation follows the reader journey (setup, then domain
  onboarding, then reference), and new orientation pages cover platform
  configuration, domain prerequisites, and what an ACTIVE domain provides.
  [Proposal](openspec/changes/publish-docs-site/proposal.md).
- Fix the guardrail SCP resource selectors: bus and queue patterns no longer
  embed the deployer's account ID (`AWS::AccountId`), so the deny ceiling
  actually matches member-account resources. Redeploy `konductor-guardrails`
  stacks created before this fix. Enforcement verified in an authorized
  [sandbox run](openspec/changes/archive/2026-09-29-protect-domain-managed-resources/sandbox-acceptance-evidence.md).
- Add the customer platform configuration flow: a published
  [configuration contract](docs/reference/configuration.md), template
  parameter constraints, a `make validate-config` preflight with a
  fixed-at-setup drift guard, and customer values moved out of committed
  deploy configuration into gitignored `konductor-platform.toml`.
  [Decision](docs/decisions/configure-customer-platform.md).
- Allow a registration to pin individual environments to AWS accounts other
  than the domain's owner account (`environmentAccounts`): validation and
  sparse storage on the domain record, per-environment resolution through
  provisioning, rollback, retagging, and the approval email, plus override
  fields in the console request form and the effective map on the approval
  card and domain detail. The trust stack must be deployed once per distinct
  account used.
  [Decision](docs/decisions/per-environment-domain-accounts.md).
- Reorganize documentation around reader tasks, focused architecture topics,
  reference pages, and operations runbooks.
- Preserve decision history in [topic-indexed records](docs/decisions/README.md)
  and align agent instructions with [documentation maintenance](docs/development/documentation.md).
- Track existing [technical discrepancies](docs/reference/documentation-discrepancies.md)
  separately without changing application or infrastructure behavior.

See the [organization decision](docs/decisions/documentation-organization.md).

## 2026-09-23

- Apply the documentation style and clarity pass, without intended technical
  changes (`40bfdd5`).

## 2026-09-18

- Harden the append-time schema compatibility gate to fail closed when its
  bounded proof cannot establish compatibility (PR #37, `a7ac2e1`).
  [Decision and pairwise concurrency limit](docs/decisions/harden-schema-compatibility-checker.md).
- Move AWS SDK `test-util` features from normal workspace dependencies to
  explicit member development dependencies (PR #39, `cd5b4b8`).
  [Rationale and measurement limits](docs/decisions/sdk-test-util-scoping.md).
- Add a separately authorized organization SCP package and ownership-aware
  resource links in the console (PR #40, `7ef04b3`).
  [Decision](docs/decisions/protect-domain-managed-resources.md).
  The account-selector issue ([D09](docs/reference/documentation-discrepancies.md#d09))
  and live sandbox acceptance remain unresolved; this entry does not claim
  enforcement.
