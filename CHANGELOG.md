# Changelog

Notable repository changes, with links to their rationale and limits.
Dates below are merge or commit dates, not release or deployment dates.
This log starts with recent changes; it is not a reconstructed release history.
Older rationale is available through the [decision index](docs/decisions/README.md).

## Unreleased

- Prepare the repository for outside contributors. The issue chooser
  offers bug report, feature request, documentation, and question forms,
  with blank issues turned off and links to private vulnerability
  reporting and the documentation site. No form asks for a priority, and
  the bug form warns reporters to remove secrets. New area labels cover
  schema management, the control plane, the web console, and the SDK, and
  Dependabot now applies labels that exist. `main` is protected by two
  rulesets kept in `.github/rulesets/`: code-owner review with squash-only
  merges, and required CI checks with linear history and no force pushes.
  `scripts/tests/test_repository_config.py` checks these files against
  each other, so the scripts test command now adds `--with pyyaml`. See
  [the decision](docs/decisions/branch-rulesets-and-issue-forms.md) and
  [contributing](CONTRIBUTING.md#issues-and-triage).
- Fix `make teardown-domain` for domains still in `PENDING_APPROVAL`. The
  script deleted the record while the domain's approval workflow kept
  waiting for a decision, so the workflow's later rejection or 7-day
  timeout wrote the record back as `REJECTED`, or rejected a domain
  re-registered under the same name. The script now stops the approval
  workflow and confirms that it ended before it deletes anything, and
  keeps the record when it cannot. See
  [cleaning up domain resources](docs/operations/domain-recovery.md#cleaning-up-domain-resources)
  and [D10](docs/reference/documentation-discrepancies.md#d10).
- Describe subscription removal's zero-events alarm deletion as best
  effort. A failed alarm deletion is not retried and the subscription
  still becomes `REMOVED`, so domain cleanup now checks every environment
  for a leftover alarm. The alarm treats missing data as breaching and
  would otherwise stay in the `ALARM` state. See
  [cleaning up domain resources](docs/operations/domain-recovery.md#cleaning-up-domain-resources)
  and [D30](docs/reference/documentation-discrepancies.md#d30).
- Correct three procedures and a troubleshooting row. Domain teardown now
  starts by removing the domain's subscriptions in both directions. The
  teardown does not remove subscriptions, so a consumer's inbound
  subscriptions were left `ACTIVE`, with their rules on the producers'
  buses. Domain recovery also removes the rules on each environment's
  application bus before a teardown re-run: Konductor creates none there,
  but a team's own integration rules block deleting that bus, as
  subscription rules block the platform bus. The uninstall guide empties
  the system-event failure bucket as well as the web console bucket before
  it deletes the core stack, and waits for the guardrail stack deletion to
  finish before it deletes the trust stacks. The subscription guide and
  architecture page limit re-enabling a `FAILED` environment to `ACTIVE`
  subscriptions; a subscription that failed as a whole is retried with a
  new `POST /subscriptions`. The authentication guide now gives the symptom
  of a missing owner binding as `404` on describe or a domain missing from
  the list, not `403`. See
  [cleaning up domain resources](docs/operations/domain-recovery.md#cleaning-up-domain-resources).
- Refuse a domain registration whose `costCenter` fails the governed-tag
  rule (empty, over 256 characters, or containing control characters)
  before anything is written. The handler checked the value inside the
  resource-name collision preflight, so an invalid value was recorded as a
  `REJECTED` domain with `decidedBy: system` and used one of the
  deployment's 1,000 domain slots. It now emits `RegisterDomainFailed` with
  no record, no counter charge, and no workflow, as the update command
  does. Rows recorded this way before the fix stay `REJECTED` and keep
  their slot. See
  [registration admission](docs/architecture/domain-lifecycle.md#registration-admission)
  and [D28](docs/reference/documentation-discrepancies.md#d28).
- Document the three registration outcomes in the domain guide, domain
  lifecycle, and domain statuses pages: refused, which changes no record
  (`GET` keeps returning `404` unless an earlier record holds the name),
  recorded as `REJECTED` by the resource-name collision check
  (`decidedBy: system`), or accepted for approval. The guide also
  shows how to follow the outcome on each API surface: until approval
  creates the owner binding, OAuth team callers get `404` from
  `GET /domains/{domainName}` and see pending or rejected registrations
  in `GET /domains`. The domain statuses page replaces its outdated text
  diagram with a pointer to the state diagram. See
  [what happens next](docs/guides/domains.md#what-happens-next).
- Organize the documentation site by audience: application-team guides
  first, then platform administrator guides, then concepts, reference, and
  contributor pages. The home page follows the same order, six page titles
  now match their navigation labels (for example, "Deploy domain-account
  trust", "System event recovery", and "Publish events"), and forwarding
  pages point to the renamed headings. The history pages (`docs/history/`)
  are no longer published; links to them open on GitHub. See
  [documentation maintenance](docs/development/documentation.md#publish-to-the-documentation-site).
- Correct procedures found in a second documentation review. The
  authentication guide's token-decode command now converts the unpadded
  base64url payload before decoding (the previous `base64 -d` pipeline
  could fail silently), domain recovery finds stuck domains with
  `make list-domains`, the guardrail procedure links its sandbox procedure
  and enforcement matrix, and API examples quote `"${AWS_REGION}"`
  consistently. See
  [decoding and inspecting a token](docs/guides/authentication.md#decoding-and-inspecting-a-token).
- Fix domain registration on installs without notifications. With either
  notification address empty, the approval workflow invoked a placeholder
  function, so every registration failed into `APPROVAL_NOTIFICATION_FAILED`
  instead of waiting for a decision. The approval request function now
  deploys regardless and, with notifications disabled, stores the task token
  without sending email, so registrations wait in the console's pending
  approvals inbox. See
  [domain recovery](docs/operations/domain-recovery.md#approval_notification_failed).
- Fix `make teardown-domain` for domains without a `/namespace/{domainName}`
  parameter, such as never-provisioned domains and re-runs after a partial
  teardown. SSM rejects deleting a missing parameter, so the script stopped
  before deleting the domain record. It now treats `ParameterNotFound` as
  already deleted and still keeps the record on any other SSM error. See
  [cleaning up domain resources](docs/operations/domain-recovery.md#cleaning-up-domain-resources).
- Stop publishing the documentation discrepancy register and its "Review
  pending" banners. The register stays in the repository as a contributor
  reference, and the pages that carried banners now describe shipped
  behavior: `owningTeam` at registration, authenticated approval and its
  7-day timeout, the hosted-UI and SRP sign-in flows, Insomnia's built-in
  OAuth setup, per-environment subscription activation, the three
  trust-stack role ARNs, OAuth-only entitlement administration, which API
  routes are queued, retained resources at uninstall, and the domain counter
  at teardown. Resolutions are dated in the
  [register](docs/reference/documentation-discrepancies.md). See
  [documentation maintenance](docs/development/documentation.md#before-changing-behavior).
- Standardise the installation and uninstall guides on make targets. The
  notifications stack deploys with `make deploy-notifications`, and removal
  runs `make list-domains`, `make teardown-core`, `make teardown-trust`,
  and `make teardown-notifications`; the AWS CLI stays for read-only checks
  and manual cleanup. See
  [documentation maintenance](docs/development/documentation.md#show-commands-in-procedures).
- Fix the platform deploy targets so `sam deploy` reaches the account and
  Region the preflight checked: `deploy-core` and `deploy-notifications`
  now pass `--region`, and the platform deploy and teardown targets pass
  `--profile` whenever `PLATFORM_PROFILE` is set. `PLATFORM_PROFILE` no
  longer defaults to the `default` profile. Unset, the targets use your
  current AWS credentials (`AWS_PROFILE`, credential environment variables,
  or the default profile), because an explicit `--profile default` would
  override `AWS_PROFILE` and the credential environment variables. Without
  `AWS_REGION`, the targets now resolve the Region in the AWS CLI's order:
  `AWS_DEFAULT_REGION`, then the Region configured for `PLATFORM_PROFILE`
  or the current profile, then `us-east-1`. The previous default ignored
  `AWS_DEFAULT_REGION` and the platform profile's Region. See
  [credentials and Region](docs/operations/installation.md#credentials-and-region).
- Add `make docs-serve` to preview the documentation site with live reload
  in the `squidfunk/mkdocs-material` container pinned by
  `requirements-docs.txt` (loopback only; `DOCS_PORT` sets the host port).
  See [documentation maintenance](docs/development/documentation.md#publish-to-the-documentation-site).
- Refresh the README overview. The How It Works diagram now shows
  first-environment schema authoring and promotion, validation at publish
  admission, re-pinning on promotion, console-or-API approval,
  per-environment domain accounts, and opt-in system events. Current Scope
  drops two claims that shipped work had made false (no schema promotion;
  business payloads not validated), and the README part of
  [D08](docs/reference/documentation-discrepancies.md#d08) is marked
  resolved.
  [Proposal](openspec/changes/refresh-readme-overview/proposal.md).
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
