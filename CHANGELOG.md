# Changelog

Notable repository changes, with links to their rationale and limits.
Dates below are merge or commit dates, not release or deployment dates.
This log starts with recent changes; it is not a reconstructed release history.
Older rationale is available through the [decision index](docs/decisions/README.md).

## Unreleased

- Fix a latent OAuth authorizer bug that denied sibling paths with a
  confusing IAM-style 403 for up to five minutes. API Gateway caches the
  authorizer's policy under (token, method, route template) — so the
  policy named one literal path — and every other path on the same route
  (browsing a second domain's detail, for example) replayed that
  path-exact policy and failed. The policy resource now wildcards the
  resource segment, mirroring the existing stage wildcard; authorization
  semantics are unchanged because the route gate is method-level and every
  handler re-checks bindings per request.

- Apply the same summary treatment to the domain page's remaining audience
  panels: Outbound subscriptions and Access grants each show at most the
  five most recently created entries, newest first, and link to new
  dedicated views (`/domains/{domain}/subscriptions`,
  `/domains/{domain}/grants`) listing every entry. The grants view keeps
  the full management surface — granting and revoking work there too.

- Report each catalog schema's most recent modification time as
  `lastModifiedAt` — the newest version-creation date across the schema's
  environment registries, omitted when no registry reports one — and cap
  the domain page's schemas panel at the five most recently modified
  schemas, with a link to a new dedicated per-domain schemas view that
  lists and pages through all of them.

- Change the header's System events navigation item to its bell icon alone,
  dropping the visible text label; the link keeps an accessible name and
  the unread indicator for screen readers and the tests that target it by
  role.

- Change the console's domain-request form to ask for one required account
  per configured environment instead of an owner account plus optional
  per-environment overrides. The two concepts were easy to conflate; teams
  now pin every environment explicitly, and the form derives the record's
  owner account from the authoring (first) environment's account. The
  register API is unchanged — direct API callers still send `ownerAccountId`
  with optional sparse `environmentAccounts` overrides. Form validation now
  gates every rule through the inline errors (native browser validation is
  disabled), so missing and malformed fields render identically.

- Fix platform roles seeing an empty schema catalog when a consumer domain
  is selected: the catalog list applied the consumer domain's entitlement
  filter to every caller, so a `platform-admin` browsing the console's
  catalog with a consumer domain chosen saw only that domain's currently
  entitled schemas (nothing, on a fresh install). Platform roles now
  bypass the entitlement filter on every catalog request, with or without
  a `consumerDomain` parameter, matching the entitlement spec's platform
  bypass clause and the schema detail route's existing behavior; the
  parameter still scopes the consumer view for non-platform callers.

- Fix the web console offering no way to register a second schema: the
  schemas panel showed "Register schema" only while the list was empty,
  so after the first registration the affordance disappeared even though
  the web-ui spec requires it for every caller with a publisher binding.
  The action is now always available on a readable panel. The schema
  authoring modal also gained the new `lg` size variant of `NxModal`
  (`max-w-2xl` instead of `max-w-lg`) for more comfortable JSON editing.

- Fix the deployed web console rendering a blank page after the Nuxt 4.6
  upgrade: Nuxt now resolves its entry through an `#entry` import map, and
  the post-build step that externalizes inline scripts (for the strict
  `script-src 'self'` CSP) rewrote that import map into a `<script
  type="importmap" src=...>` tag. Browsers ignore `src` on import maps
  (they are inline-only per the HTML spec), and the strict CSP blocks the
  inline form, so the entry never loaded. The post-build step now removes
  the import map entirely and rewrites every `#entry` specifier in the
  emitted chunks to a plain relative import of the sibling entry file;
  the suite gained unit tests and the build fails loudly on unexpected
  import-map shapes instead of shipping a blank page.

- Fix `teardown-domain` leaking the domain's `/namespace/{name}` SSM
  parameter and failing to delete the system-events topic: the script now
  removes the parameter alongside the record (a leftover parameter makes
  re-registration fail the collision check), and its topic-manager
  invocation passes `--cli-binary-format raw-in-base64-out`, which AWS CLI
  v2 requires for a raw JSON payload.

- Fix failed domain provisions leaking the `/namespace/{domain}` SSM
  parameter: the workflow's failure paths compensated the system-events
  topic but never the parameter, so the registration collision check then
  blocked re-registration of the domain ("resource name collision: SSM
  parameter ...") until someone deleted it by hand. Both failure paths now
  delete the parameter after the topic, and the workflow role gained
  `ssm:DeleteParameter`.

- Fix domain provisioning failing while setting the per-domain
  system-events topic policy: SNS now rejects topic policies with wildcard
  or non-topic-scoped actions ("Policy statement action out of service
  scope"), so the owner grant's `sns:*` (and `sns:Unsubscribe`, which is
  subscription-scoped) made every provision roll back at the
  `EnsureSystemTopic` step. The owner grant now names its topic-scoped
  management actions explicitly; publishing stays IAM-governed and
  unsubscription goes through the topic manager's IAM grant.

- Fix a create-ordering race that can fail a fresh core-stack deploy: state
  machines referenced their role but not the `AWS::IAM::Policy` that grants
  the role its permissions, so CloudFormation could create a state machine
  before the policy attached. Step Functions validates the `.sync`
  managed-rule permission at create time and rolled the whole stack back
  when it lost the race. Every state machine now `DependsOn` its role's
  policy resources.

- Fix the system-event projector's event source mapping failing a fresh
  core-stack create: the function's role could `PutObject` into the
  failure bucket but not `ListBucket` the bucket itself, and Lambda
  validates that grant while creating the mapping's S3 on-failure
  destination, rolling the whole stack back. The role now carries the
  bucket-level read.

- Fix the system-event schema bootstrap failing a fresh core-stack create:
  the function's `SYSTEM_SCHEMA_REGISTRY` variable and its IAM resources
  used `!Ref` on the registry, which renders as the registry *ARN* rather
  than its name, so the bootstrap's first `DescribeSchema` was denied and
  the whole stack rolled back. Both now spell out the deterministic
  registry name.
- Fix the system-event projector failure bucket exceeding S3's 63-character
  name limit: the bucket is now
  `konductor-system-event-failures-{account}-{region}` with no organization
  prefix, so every organization the configuration contract allows fits in
  every region. Previously a core-stack create in a long-identifier region
  (for example `ap-southeast-2`) rolled the whole stack back.
- Reject `multipleOf` divisors above 9007199254740991 (2^53 - 1) on
  `POST /schemas` and `POST /schemas/versions` with `400 Bad Request`. The
  validator cannot decide divisibility exactly above that bound, so such
  schemas could admit non-multiples or reject true multiples. Already
  stored schemas keep validating at publish. See the
  [decision record](docs/decisions/refuse-inexact-multiple-of-divisors.md).
- Add domain system events: the platform projects committed subscription
  upgrade phases onto a central system bus as versioned public contracts
  (`SubscriptionUpgradeStarted`, `SubscriptionUpgraded`, source
  `konductor.system`) and routes each event to the recipient domain's
  platform-owned SNS topic `{org}-{domain}-system-events` (SQS, Lambda, and
  email subscriptions, restricted to the domain's own accounts; the domain
  owner email is subscribed by default at provisioning and re-pointed when
  the owner changes). Consumers
  deduplicate on the deterministic `idempotencyKey` and order on
  `resourceRevision`; a 90-day bus archive is audit-only with no replay.
  Topic ARNs appear on `GET /domains/{domainName}` and on the console's
  top-level **System events** page; the router records a latest-event marker
  (`systemEventsLastEventAt`/`systemEventsLastEventType`) on the domain,
  which the console's nav uses for a browser-local unread dot. Existing
  domains gain topics via
  `scripts/backfill-system-event-topics.sh`. When SES notifications are
  enabled, owners also get advisory emails for promotion-triggered upgrades
  and promotion failures. Emission is gated by the new
  `SystemEventEmissionEnabled` stack parameter (default off).
- Add `GET /schemas/{schemaName}/content` on both API surfaces: publishers
  (and platform roles) read one schema version's content from one
  environment's registry, defaulting to the environment's latest. The
  producer schema page opens the content in a viewer from pipeline nodes
  (environment latest) and version-history rows (the row's version).
- Fix OAuth authorizer caching denying console requests after an
  environment switch: cached policies were scoped to one stage's method
  ARN while the cache key omitted the stage, so the same route on another
  stage was denied for up to five minutes without invoking the authorizer.
- Add explicit cross-environment schema promotion: `POST /schemas/promotions`
  on a target stage commits a promotion head item (idempotent, `409` while one
  is active), a promotions-table stream consumer starts the
  `PromoteSchemaWorkflow` state machine (idempotent execution name), and the
  workflow replays the missing versions into the target registry under the
  same version numbers, validating the shared prefix first (`DIVERGED` stops
  without touching the target; `REPLAY_FAILED` resumes where it stopped).
  The workflow also refuses a source that is not the target's immediate
  predecessor (`SKIPPED_ENVIRONMENT`), gating executions started out-of-band.
  Schemas authored before version recording existed replay from the source
  registry's immutable per-version content, so they promote without any
  backfill.
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
