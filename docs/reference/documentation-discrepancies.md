# Documentation discrepancies

[Documentation home](../README.md)

These items were identified while reorganizing the documentation from commit
`40bfdd5`. They remain **open for separate technical review**. The reorganization
does not change application behavior, repair templates, or establish live
deployment guarantees.

Conflicting passages are retained with warnings or as explicitly historical
evidence. Do not copy them into new requirements. To close an item, record the
applicable requirement, implementation/test evidence, and authorized correction.
Historical decisions retain their original rationale with a supersession link.

## D01

**Subscription activation semantics.** The [consumer guide](../guides/subscriptions.md#subscribing)
says activation fails as a whole if any environment lacks the schema.
[Subscription architecture](../architecture/subscriptions.md#subscription-flow)
describes independent materialization and `AWAITING_PROMOTION`, without
cross-environment compensation.

The [promotion-following design](../../openspec/changes/archive/2026-09-08-follow-schema-promotion/design.md)
records the later decision explicitly. Review the guide against the subscription
requirement and materialization tests before changing its instructions.

## D02

**Approval links, timeouts, and notification recovery.** The
[domain guide](../guides/domains.md#what-happens-next) describes approval/rejection
email links, while [authorization](../architecture/authorization.md#identity-and-authorization)
describes authenticated POST-only approval. Token examples and troubleshooting in
the [authentication guide](../guides/authentication.md) still refer to email tokens.

The domain guide's timeout paragraph says `PENDING_APPROVAL` remains; the
[recovery guide](../operations/domain-recovery.md#domain-stuck-in-pending_approval)
and [workflow explanation](../architecture/domain-lifecycle.md#approval-workflow)
describe a rejected timeout. The recovery guide says no owner email follows an
approval-notification failure, while the workflow explanation lists a delayed
registration email. The [dated notification audit](../development/side-effects/domain-management.md)
describes another failure/timeout sequence.

Review the approval state machine, notification handlers, and authenticated
inbox together. Manual deletion recommendations also depend on D10.

## D03

**Trust-role parameters and permissions.** The earlier
[trust deployment recipe](../operations/trust-stack-deployment.md#manual-alternative)
names two platform roles. The same page preserves the
[installation handoff](../operations/trust-stack-deployment.md#installation-handoff-three-role-recipe),
which additionally requires `RetagWorkflowRoleArn`.
[Trust architecture](../architecture/cross-account-trust.md) also describes two
roles and retained bus-to-bus permissions.

Do not select one recipe by implication. Reconcile the guide, Makefile outputs,
and [trust template](../../trust-stack.yaml) in a separate change, including
retagging and the distinction between unused permissions and actual delivery.

## D04

**Test features and disk estimates.** The
[mock setup example](../development/testing.md#setup) enables SDK `test-util` in
normal workspace dependencies. The merged [development-only feature decision](../decisions/sdk-test-util-scoping.md)
records the opposite placement.

The [disk guidance](../development/testing.md#keeping-target-small) also gives
crate counts, build sizes, and savings without a matched before/after benchmark.
Do not use those figures as measured savings. Update examples against the
manifests separately; retain production and test workload distinctions in any
future measurement.

## D05

**Authorization surfaces and copied endpoint inventories.**
[Entitlement architecture](../architecture/entitlements.md) describes a
platform-operated IAM administration surface. The
[entitlement decision](../decisions/add-subscriber-entitlements.md) describes
OAuth-only administration and producer self-service.

The [old endpoint tables](../history/documentation-baseline.md) also contain
role requirements that differ from binding-based descriptions. They have been
removed from active reference pages, not silently corrected.
Review [API contracts](api.md), authorization requirements, handlers, and tests
before reconciling permissions. A route inventory alone does not establish a
grant-engine decision.

## D06

**Authentication examples.** The
[CLI password flow](../guides/authentication.md#option-b-cli-admin-auth-flow)
is presented as usable, while the SRP section says raw-password authentication
is disabled. The “direct login, no redirect” section describes token return but
shows a code redirect and exchange.

Live confirmation on 2026-10-22 (fresh ap-southeast-2 install): the user-pool
client enables only `ALLOW_USER_SRP_AUTH` and `ALLOW_REFRESH_TOKEN_AUTH`, so
Option B's `ADMIN_USER_PASSWORD_AUTH` call is rejected by the client, and
Option C's `redirect_uri=http://localhost` fails unless the operator's
`oauth_callback_urls` includes it (the committed example lists only
`http://localhost:3000/callback`). With no browser, the only working token
path for the smoke test is an SRP client (for example `pycognito`) or the
hosted-UI form. User setup also states a 12-character minimum but includes a
shorter temporary-password example. Review the guide against the deployed
client configuration; either enable `ADMIN_USER_PASSWORD_AUTH` for operator
CLI use or replace Option B with a documented SRP helper.

## D07

**Insomnia script.** The [Insomnia example](../guides/api-clients.md#using-insomnia)
mixes `insomnia.*` calls with `pm.environment.get`, a Postman API.
Review the supported client scripting APIs before treating it as runnable.

## D08

**Stale broad summaries and inventories.** The previous README stated:
“Business payloads are not validated against schema content.” That conflicts
with the [admission design](../architecture/publish-validation.md). The new README
links to topic pages instead of repeating that scope claim.

The [old project tree and technology table](../history/documentation-baseline.md#infrastructure-as-code)
include `api/openapi.yaml` and a “single table” description. The
[detailed topology](../architecture/platform-boundaries.md) says writes are
asynchronous despite documented synchronous paths. The
[quota inventory](../operations/service-quotas.md#resource-consumption-model)
and [uninstall inventory](../operations/uninstall.md) also need a current resource census.
Original “12+ resources” and “1–2 days” statements are preserved as historical
context, not validated measurements.

OpenSpec's former embedded context included “single-table design,” “at least
30 days” of domain DLQ retention, and subscription-created IAM/resource policies.
Those summaries now yield to topic links. This does not decide their intended
requirements or certify the deployed retention/topology.

## D09

**Guardrail account selectors and enforcement evidence.** — **Resolved 2026-09-29.**

Original finding: the [guardrail template](../../guardrail-stack.yaml) used
`${AWS::AccountId}` in bus and queue resource selectors. CloudFormation
resolves it to the stack's account, not each SCP attachment target, so a
management-account stack selected that account's resources rather than the
member-account buses and queues. The
[local fixtures](../../lib/konductor-common/tests/guardrail_fixtures.rs)
replaced the value with `*`, so their evaluation was not
deployment-equivalent.

Resolution: the template selectors are account-agnostic
(`arn:aws:events:*:*:event-bus/${Organization}-*`,
`arn:aws:sqs:*:*:${Organization}-*`) and a raw-template regression test
(`resource_selectors_are_account_agnostic`) locks the wildcard form. Live
enforcement was verified in the authorized
[sandbox acceptance run](../../openspec/changes/archive/2026-09-29-protect-domain-managed-resources/sandbox-acceptance-evidence.md)
(13 denied operations, each attributed by AWS to the enforcing SCP). The
fixture evaluator's `AccountId → *` normalization remains as an evaluation
convenience and is not cited as enforcement evidence. One residual from the
same family: `DenyPolicyVersionRollback` still binds `AWS::AccountId`; see
the coverage limits in the [guardrail runbook](../operations/guardrails.md).

## D10

**Domain writes, teardown, and the counter.** The
[registration guide](../guides/domains.md#what-happens-next) describes a conditional
`PutItem`; the [domain-cap decision](../decisions/cap-domain-count.md) and
[storage reference](data-model.md#domain-count-ceiling) describe a transaction
with a counter.

The [cleanup guide](../operations/domain-recovery.md#cleaning-up-domain-resources)
describes deleting domain rows, while the [quota runbook](../operations/service-quotas.md#the-1000-domain-ceiling)
says rows are never deleted and the counter never decrements.
Review teardown/re-registration behavior and counter reconciliation together,
before following destructive recovery advice.

## D11

**Examples and status-code typography.** The domain response example contains
abbreviated, non-JSON `{ "..." }` objects and an `ownerAccountId` that differs
from its application-resource ARNs. The simplified
[producer envelope example](event-envelope.md#producer-obligations-the-event-envelope)
uses an object `Detail`, unlike the serialized `PutEvents` entry shown above it.
These examples were preserved, not promoted to executable fixtures.

The earlier style pass also left `409 s`, `400 s`, and `503 s` in some HTTP
failure prose. Review these as status-code plurals, not timing claims. No
runtime or latency meaning is inferred from them during this reorganization.

## D12

**SES parameter names in the architecture spec.** The
[system-architecture spec](../../openspec/specs/system-architecture/spec.md)
names `ApprovalFromEmail` and `ApprovalToEmail` as the SES parameters. The
[core template](../../konductor-core-stack.yaml) declares
`NotificationSenderEmail` and `PlatformOperatorEmail`; the spec's names exist
nowhere in templates or code. The `configure-customer-platform` change uses
the template names. Review whether the spec requirement should be corrected to
the implemented names; not resolved here.
## D13

**Resource naming convention versus deployed names.** The naming convention
(`{org}-{env}-{domain}` and `{org}-konductor-*` prefixes in project context)
does not match the deployed core template, which uses unprefixed names such as
`konductor-event-streams` and `konductor-bus`. The `domain-system-events`
change names its new resources `{org}-konductor-system-bus`,
`{org}-konductor-system-schemas`, and `{org}-konductor-system-events-archive`
per the convention while leaving every existing unprefixed resource unchanged.
Review whether existing resources should migrate to the prefixed convention;
renaming them is destructive and not authorized here.

## D14

**Registry serves more than the latest version.** The
[schema architecture](../architecture/schemas.md) describes the EventBridge
schema registry as serving the latest version only. Promotion replay, consumer
re-pinning, and per-environment content reads all address historical versions
by number through `DescribeSchema`, and the `{org}-konductor-system-schemas`
registry relies on the same versioned reads. Review the architecture text
against the versioned-read implementation; not resolved here.

## D15

**Domain DLQ retention exceeds the SQS maximum.** Project context and
operational guidance ask for 30-day retention on domain dead-letter queues,
but SQS supports at most 14 days of message retention. The system-event
failure paths use 14-day SQS queues (the supported maximum) plus 90-day S3
payload retention for the projector, and do not claim to satisfy a 30-day
promise. Review the 30-day requirement against the SQS limit and either adjust
the expectation or add a long-retention mechanism in a separate change.

## D16

**Playwright suite is not gated in CI.** AGENTS.md → Web UI requires new
interactive surfaces to ship Playwright coverage, and the ui-reviewer droid
previously listed `pnpm test:ui` as a CI-enforced gate. CI's Web job runs
`pnpm lint`, `pnpm typecheck`, and `pnpm build` only, so the suite runs only
when a contributor runs it locally. Review whether to add `pnpm test:ui` (with
a Playwright browser install) to the Web job.

## D17

**Platform resource names in AGENTS.md versus the resource-topology spec.**
AGENTS.md → Product Naming lists `konductor-trust-role` and `konductor-bus`,
which match the deployed core template. The
[resource-topology spec](../../openspec/specs/resource-topology/spec.md)
requires `{org}-konductor-trust-role` and `{org}-konductor-*` platform
resources. This is the agent-facing side of [D13](#d13); decide which naming
is authoritative before changing either.

## D18

**Feature-branch workflow versus practice.** AGENTS.md → Git Workflow calls
for short-lived feature branches per change. The promote-schema-versions,
schema-content-per-environment, and domain-system-events changes were
committed directly on `main` (12 commits ahead of `origin/main` on
2026-10-21). Review whether the rule or the practice should change.

## D19

**`null` rule versus console code.** The
[TypeScript conventions](../development/typescript.md) say `undefined` means
absent and `null` is introduced only where the API contract uses it. The
console uses `null` about 127 times, including 25 `ref<T | null>(null)` UI
state declarations, and Vue template refs (`useTemplateRef`) return `null`
by framework contract. Review whether to amend the rule (for example, allow
`null` for UI selection state and framework refs) or migrate the code.

## D20

**Radius and raw-control exceptions.** The
[design system](../development/design-system.md) and AGENTS.md allow only
`rounded-lg` and `rounded-xl`. The console also uses `rounded-full` (the
System events unread dot in `layouts/default.vue` and the `NxButton`
spinner), `rounded-md` (the settings menu item in `layouts/default.vue`), and
plain `rounded` (`MetadataEnvironmentPanel.vue`). The raw-control exception
list does not name the environment disclosure button on the domain page
(`aria-expanded`) or the settings menu trigger (`aria-haspopup`). Review
whether to document these as exceptions or change the code.

## D21

**Lifecycle-event catalog omits `konductor.control-plane`.** The
metadata-stream-consumer and entitlement-stream-consumer publish to
`konductor-bus` with source `konductor.control-plane`, but the
[lifecycle-event catalog](lifecycle-events.md) lists no events for that
source. Review the catalog against both consumers.

## D22

**File-size guideline versus current code.** AGENTS.md → Code Style asks for
files under 300 production lines (excluding `#[cfg(test)]` modules), and the
rust-reviewer droid uses its own per-type thresholds (300 lines for Lambda
handlers, 400 for shared-library modules). On 2026-10-21, 13 Rust files
exceeded 300 production lines (non-blank, non-comment lines outside test
code):

| File | Lines |
| ---- | ----: |
| `event-streams/functions/publish-events-admission/src/handler.rs` | 814 |
| `domain-management/functions/domain-registrations/src/register.rs` | 623 |
| `event-streams/functions/schema-admin/src/handler.rs` | 538 |
| `lib/konductor-common/src/streams_db.rs` | 459 |
| `domain-management/functions/list-domains/src/handler.rs` | 383 |
| `event-streams/functions/materialize-subscription-env/src/handler.rs` | 377 |
| `event-streams/functions/materialize-subscription-env/src/repin.rs` | 369 |
| `lib/konductor-common/src/schema_compatibility/mod.rs` | 359 |
| `event-streams/functions/describe-schema/src/contract.rs` | 348 |
| `event-streams/functions/subscriptions/src/update_state.rs` | 341 |
| `event-streams/functions/system-schema-bootstrap/src/handler.rs` | 337 |
| `lib/konductor-common/src/grant_engine.rs` | 328 |
| `event-streams/functions/subscriptions/src/subscribe.rs` | 312 |

A `make check` gate with an allowlist of these files was tried and removed
the same day; the rule was relaxed to a review guideline instead. Assess
whether to split the files by role (most separate cleanly; the
publish-events admission handler needs boundary design), keep the guideline,
or change the limit, and align AGENTS.md with the reviewer thresholds.

## D23

**Uninstall inventory drifts from the deployed core stack.** A full teardown
and reinstall on 2026-10-22 (ap-southeast-2, org `acme`) found the
[uninstall procedure](../operations/uninstall.md) out of step with the
deployed template in four ways:

- The only `DeletionPolicy: Retain` resource in the deployed core stack is
  the Cognito user pool (`${Organization}-konductor-users`). The uninstall
  procedure never mentions it. The pool orphans on stack delete, a reinstall
  creates a *new* pool (Cognito pool names are not unique, so nothing
  collides — the old pool is simply stranded with its users), and no
  adoption path exists for reattaching an existing pool to a new stack.
  Operators who want identity continuity across a reinstall have none.
- The procedure says the `konductor-domains` table is "retained by
  `DeletionPolicy`" and must be deleted manually. The deployed template
  declares `DeletionPolicy: Delete` on all seven DynamoDB tables; the table
  went with the stack, and the manual step is stale.
- The procedure omits emptying the web console bucket
  (`{org}-konductor-web-{account}-{region}`). CloudFormation cannot delete a
  non-empty S3 bucket, so a stack delete with the published build in place
  hangs indefinitely. `make deploy-web` publishes that build, and nothing in
  the teardown path removes it.
- The procedure omits the optional `konductor-guardrails` stack (three
  organization SCPs) from its removal sequence, even though the
  [installation path](../operations/installation.md#phase-3-onboard-a-domain-account)
  links to its deployment.

Review the uninstall inventory against the deployed template and either
correct the procedure or change the template's retention policies; also
review whether Cognito pool adoption (an optional existing-pool setting in
the platform configuration) is wanted as a supported reinstall path.

## D24

**Domain deprovision cannot remove schemas or subscription rules.** During
the same teardown, `make teardown-domain` failed for every domain that had
used its registry or subscriptions. The `deprovision-env` workflow:

- cannot delete a schema registry that still holds schemas (EventBridge
  Schema Registry returns 409 "registry you tried to delete is not empty"),
  and
- cannot delete a platform event bus that still carries materialized
  subscription rules (EventBridge returns 400 "EventBus can't be deleted
  since it has rules").

The script reports these as "resources left behind", leaves the domain
record in place, and re-runs are idempotent, so recovery is possible — but it
required deleting six schemas and four rules (targets first, then rules) by
hand across dev/staging before the three domains would tear down. Domains
whose registries were empty (prod) tore down cleanly. Review whether
deprovision-env should enumerate and delete registry schemas and bus rules
before the registry/bus steps (making `make teardown-domain` self-sufficient
for domains that used their schemas or subscriptions), or whether the
script should instruct the manual purge. Either resolution also needs an
update to the [uninstall procedure](../operations/uninstall.md), whose only
domain step assumes `make teardown-domain` suffices.

## D25

**Fixed-name resources cannot hold the contract's largest organizations.**
The [configuration contract](configuration.md) allows `organization` up to
43 characters, and `make validate-config` accepts any syntactically valid
value, but several `${Organization}`-prefixed resource names in the core
template exceed their service limits long before 43 characters. A clean
core-stack create on 2026-10-22 (ap-southeast-2) rolled back on the
system-event projector failure bucket, whose name rendered to 64 against
S3's 63-character limit; that bucket was renamed without the prefix in the
same fix. The remaining patterns, against their service name limits:

- `KonductorWebBucket` (`${Organization}-konductor-web-{account}-{region}`):
  in ap-southeast-2 the fixed portion is 42 characters, so the name breaks
  for organizations over 21 characters (in us-east-1, over 27).
- The system-events archive (`{org}-konductor-system-events-archive`):
  against a 64-character EventBridge archive-name limit, breaks for
  organizations over 26 characters.
- The system schemas registry (`${Organization}-konductor-system-schemas`):
  against a 64-character registry-name limit, breaks for organizations over
  39 characters.
- The Cognito hosted-UI domain (`${Organization}-konductor-{account}`):
  against a 63-character domain limit, breaks for organizations over 40
  characters.

The preflight does not check name-fit consequences, so a large-organization
install passes `make validate-config` and fails mid-create with a full
rollback. Review whether to shrink the contract's organization bound, make
the preflight reject organizations whose names do not fit, or shorten the
patterns; renaming any already-deployed resource is destructive and out of
scope here.

## D26

**Retained resources break create-retry after a rollback.** Two core-stack
resources carry `DeletionPolicy: Retain` — the Cognito user pool, and the
system-events schemas registry (retained, per the template comment, because
CloudFormation cannot delete a registry that holds schemas). Retention also
applies when a failed `CREATE` rolls back: the resources survive the
rollback, and re-running `make deploy-core` does not start clean.

On 2026-10-22, a first create failed late (see [D25](#d25)) and its
rollback retained the schemas registry; the retry then failed on the
registry with 409 "already exists", and each failed attempt also left an
orphaned user pool behind (Cognito pool names are not unique, so the retry
creates a *new* pool with the same name while the old one strands).
Recovery required deleting the retained registry and both orphan pools by
hand before the third attempt. Review whether the registry should adopt an
existing instance instead of failing (CloudFormation resource import, or a
bootstrap that manages it outside the stack), whether the installation
troubleshooting page should document rollback-retry cleanup, and whether a
failed-create cleanup should purge retained resources automatically.

**Update, 2026-10-22: rollback also orphans Lambda log groups, and
CloudFormation's early validation turns them into hard create failures.**
After the third failed create (bootstrap IAM bug, fixed test-first), the
fourth `deploy-core` never got past changeset creation:
`AWS::EarlyValidation::ResourceExistenceCheck` failed it on a name conflict
with `/aws/lambda/konductor-system-schema-bootstrap`, a log group the
template itself creates with `DeletionPolicy: Delete`. The rollback had
already removed the stack-owned group, but the custom resource's own
rollback/delete invocation re-triggered logging after that deletion, and
CloudWatch recreated the group outside the stack. The new pre-deployment
validation (November 2025 CloudFormation feature, on by default) then
blocked every subsequent create changeset — the changeset fails in seconds
with only the generic hook name; the per-resource detail requires the
`describe-events` API call with the change-set ARN, which neither SAM CLI
nor the current troubleshooting docs surface. Deleting the orphan log group
restored a clean create. Review whether teardown/troubleshooting should
sweep `/aws/lambda/konductor-*` log groups after any rollback (the uninstall
sequence does not), and whether `deploy-core` should preflight fixed-name
conflicts so the early-validation failure names the resource at the point
of deploy rather than after a 15-minute build.


## D27

**The domain registration field table omits `owningTeam`, which is
required for OAuth callers.** `docs/guides/domains.md` documents the
register request fields as `domainName`, `ownerAccountId`,
`environmentAccounts`, `ownerEmail`, `secondaryOwnerEmails`,
`description`, and `tags` — with no mention of `owningTeam`. The
handler (`domain-management/functions/domain-registrations/src/register.rs`,
`authorize_registration`) denies every OAuth registration that does not
name an `owningTeam` matching one of the caller's Cognito team groups:
the request is accepted by the API, fails inside the command handler with
a `RegisterDomainFailed` outcome, and no domain record appears. On
2026-10-22, seeding three team-owned domains exactly as the guide's field
table prescribes produced three silent authorization denials; the
registrations only succeeded once `owningTeam` was added. The guide
should either add the field to the table (required for OAuth callers,
optional for IAM callers, where omitting it means approval creates no
owner binding) or split the OAuth and IAM request examples.
