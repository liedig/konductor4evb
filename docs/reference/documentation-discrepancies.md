# Documentation discrepancies

[Documentation home](../README.md)

These items were identified while reorganizing the documentation from commit
`40bfdd5` and in later reviews. An item stays **open for separate technical
review** until its entry records a dated resolution. The reorganization itself
did not change application behavior, repair templates, or establish live
deployment guarantees.

This register is a contributor reference kept in the repository. It is
excluded from the published documentation site, and the customer-facing pages
(guides, operations, architecture, and reference) neither link to it nor carry
review banners; see
[documentation maintenance](../development/documentation.md#before-changing-behavior).
Do not copy a conflicting passage into new requirements. To close an item,
record the applicable requirement, implementation/test evidence, and authorized
correction. Historical decisions retain their original rationale with a
supersession link.

Resolutions dated 2026-10-25 come from two reviews of the published site. Each
one corrects the published pages against the handlers, templates, and tests
cited in its entry.

## D01

**Subscription activation semantics.** — **Resolved 2026-10-25.**

Original finding: the [consumer guide](../guides/subscriptions.md#subscribing)
said activation fails as a whole if any environment lacks the schema.
[Subscription architecture](../architecture/subscriptions.md#subscription-flow)
describes independent materialization and `AWAITING_PROMOTION`, without
cross-environment compensation.

The [promotion-following design](../../openspec/changes/archive/2026-09-08-follow-schema-promotion/design.md)
records the later decision explicitly. Review the guide against the subscription
requirement and materialization tests before changing its instructions.

Resolution: the consumer guide now describes per-environment activation. The
subscription becomes `ACTIVE` when one environment materializes. An
environment whose registry lacks the schema waits as `AWAITING_PROMOTION` and
activates when the producer promotes the schema there. The subscription fails
as a whole only when no environment can materialize. Evidence:
`schema_absent_marks_the_environment_awaiting_promotion`
(materialize-subscription-env) and
`promotion_extends_awaiting_env_and_emits_event` (subscription-extender). The
[superseded saga decision](../decisions/add-event-subscriptions.md) and the
[promotion-following decision](../decisions/follow-schema-promotion.md) note
the reconciliation.

## D02

**Approval links, timeouts, and notification recovery.** — **Resolved
2026-10-25.**

Original finding: the
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

Resolution: approval is authenticated and POST-only. Administrators decide
from the console's **Pending approvals** page or with
`POST /domains/registrations/decide`, taking the task token from
`GET /domains/registrations/pending`. Approval emails link to the console and
carry no decision links or tokens. A request with no decision after 7 days is
rejected with `decidedBy: system:timeout`. `APPROVAL_NOTIFICATION_FAILED`
means the task-token write or a non-permanent SES call failed; when
notifications are enabled, operators hear about it through the platform
notifications topic and owners receive a registration-delayed email. A
permanent SES rejection, such as an unverified identity, leaves the request in
`PENDING_APPROVAL`, still decidable from the inbox. The domain, authentication,
and recovery guides, the [workflow explanation](../architecture/domain-lifecycle.md#approval-workflow),
the [status reference](domain-statuses.md), and the dated side-effects record
now agree.

The same review found a defect: with notifications disabled, the approval
workflow invoked a placeholder function ARN, so every registration failed into
`APPROVAL_NOTIFICATION_FAILED` instead of waiting for a decision. The approval
request function now deploys regardless and, with notifications disabled,
stores the task token without sending email. Evidence:
`disabled_notifications_record_the_task_token_without_email`
(send-approval-request-notification) and
`approval_workflow_stores_task_tokens_when_notifications_are_disabled`
(template test). Requirement:
[SES configuration](../../openspec/specs/domain-owner-notifications/spec.md#requirement-ses-configuration).

## D03

**Trust-role parameters and permissions.** — **Resolved 2026-10-25.**

Original finding: the earlier
[trust deployment recipe](../operations/trust-stack-deployment.md#manual-alternative)
names two platform roles. The same page preserves the
[installation handoff](../operations/trust-stack-deployment.md#installation-handoff-three-role-recipe),
which additionally requires `RetagWorkflowRoleArn`.
[Trust architecture](../architecture/cross-account-trust.md) also describes two
roles and retained bus-to-bus permissions.

Do not select one recipe by implication. Reconcile the guide, Makefile outputs,
and [trust template](../../trust-stack.yaml) in a separate change, including
retagging and the distinction between unused permissions and actual delivery.

Resolution: the trust template takes three platform role ARNs
(`KonductorRoleArn`, `ProvisioningWorkflowRoleArn`, and
`RetagWorkflowRoleArn`), which the core stack outputs as
`KonductorFunctionRoleArn`, `ProvisioningWorkflowRoleArn`, and
`RetagWorkflowRoleArn`. The deployment guide's Makefile, manual, and update
recipes now pass all three. The trust architecture describes each role and its
actions, including the tagging actions the retag workflow uses. The template's
bus-to-bus delivery permissions are unused, and the architecture page says so
instead of implying that delivery depends on them.

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

**Authorization surfaces and copied endpoint inventories.** — **Resolved
2026-10-25.**

Original finding:
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

Resolution: entitlement administration is OAuth-only. The IAM contract has no
entitlement routes, and the `entitlement-admin` handler denies IAM principals.
A platform administrator, or a caller with a `publisher` binding on the
producer domain, can create, revoke, and list that producer's entitlements. A
consumer-scoped list also accepts a `subscriber` or `publisher` binding on the
consumer domain. `IamOnlyAuthorizer` is used only in tests. Evidence:
`iam_caller_is_denied_on_every_route`,
`publisher_self_service_is_scoped_to_its_own_producer_domain`, and
`list_by_consumer_allows_admin_and_bound_subscriber_only` (entitlement-admin).

The entitlement and [authorization](../architecture/authorization.md) pages
now match the decision. The authorization page also states the authorizer's
team-caller route gate, the `owner` role, the governed update fields, and
which routes are queued. The
[access-token claims](authentication.md#group-to-role-mapping) table no longer
says a team group grants no API access, and [API contracts](api.md) points to
the authorization page instead of this entry. The old endpoint tables stay in
the history page as dated evidence.

## D06

**Authentication examples.** — **Resolved 2026-10-25.**

Original finding: the CLI password flow (then Option B of the
[authentication guide](../guides/authentication.md#obtaining-an-access-token))
was presented as usable, while the SRP section says raw-password authentication
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

Resolution: the authentication guide keeps only flows the deployed client
allows: the hosted UI with a configured callback URL
(`http://localhost:3000/callback` by default), SRP from a script
(`pycognito`), and SRP in an application. It states that password flows are
disabled and adds a troubleshooting row for the client's error. The
[app client row](../operations/access-administration.md#what-konductor-provisions)
of the user setup page names the same flows. Its temporary-password example
(`TempPass123!`, 12 characters) already met the minimum. Evidence:
`user_pool_client_supports_srp_login` (template test).

## D07

**Insomnia script.** — **Resolved 2026-10-25.**

Original finding: the [Insomnia example](../guides/api-clients.md#using-insomnia)
mixed `insomnia.*` calls with `pm.environment.get`, a Postman API.
Review the supported client scripting APIs before treating it as runnable.

Resolution: the Insomnia steps now configure Insomnia's built-in OAuth 2.0
authorization-code flow instead of a pre-request script, so no Postman API
remains. The Postman steps are a separate section.

## D08

**Stale broad summaries and inventories.** The previous README stated:
“Business payloads are not validated against schema content.” That conflicts
with the [admission design](../architecture/publish-validation.md). The claim
remained in the README's Current Scope section after the reorganization,
next to a “no schema promotion” bullet that later shipped work also made false.

**README claims resolved 2026-10-23** (`refresh-readme-overview`): both bullets
are removed, and the README overview now states that admission validates
payloads before the `202` and that schemas are promoted from the first
environment. Evidence: [publish validation](../architecture/publish-validation.md)
and [schema management](../architecture/schemas.md#authoring-environment-and-promotion).
The remaining items in this entry stay open.

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

**Topology resolved 2026-10-25:** the
[platform boundaries](../architecture/platform-boundaries.md) page no longer
says writes are asynchronous. Only domain registration and update and
subscription commands are queued through SQS, publishing admits synchronously
before it enqueues, and other writes are synchronous Lambda calls (see the
route integrations in the OpenAPI files). Its "one Lambda per API operation"
and "no intermediate Lambdas" bullets were corrected the same way. The quota
and uninstall inventories still need a current resource census (the uninstall
page's retained-resource list was corrected under [D23](#d23)), and the
OpenSpec summaries above stay undecided, so this entry remains open.

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

**Domain writes, teardown, and the counter.** — **Resolved 2026-10-25.**

Original finding: the
[registration guide](../guides/domains.md#what-happens-next) describes a conditional
`PutItem`; the [domain-cap decision](../decisions/cap-domain-count.md) and
[storage reference](data-model.md#domain-count-ceiling) describe a transaction
with a counter.

The [cleanup guide](../operations/domain-recovery.md#cleaning-up-domain-resources)
describes deleting domain rows, while the [quota runbook](../operations/service-quotas.md#the-1000-domain-ceiling)
says rows are never deleted and the counter never decrements.
Review teardown/re-registration behavior and counter reconciliation together,
before following destructive recovery advice.

Resolution: registration writes the `PENDING_APPROVAL` row and charges the
counter in one transaction, and the registration guide now says so.
`make teardown-domain` deletes the domain's row after its resources but never
decrements the counter. The cleanup guide, the storage reference, and the
quota runbook now agree and describe reconciling the counter after deletions.
The domains table's deletion policy is `Delete`, not `Retain`.

The same review found that the teardown script stopped before deleting the
record whenever `/namespace/{domainName}` was missing (never-provisioned
domains, domains provisioned before the parameter step, and re-runs), because
SSM rejects deleting a missing parameter. It now treats `ParameterNotFound` as
already deleted and still keeps the record on any other SSM error. Evidence:
`scripts/tests/test_teardown_domain.py`.

**Updated 2026-10-26:** a pull-request review found that the teardown script
deleted a `PENDING_APPROVAL` domain's record while its approval workflow kept
waiting for a decision. The workflow's later rejection or 7-day timeout wrote
the record back as `REJECTED`, or rejected a domain re-registered under the
same name. The script now stops the `approval-{requestId}` execution and
confirms it ended before it deletes anything, and keeps the record when it
cannot. Evidence: `scripts/tests/test_teardown_domain.py`.

## D11

**Examples and status-code typography.** — **Resolved 2026-10-25.**

Original finding: the domain response example contains
abbreviated, non-JSON `{ "..." }` objects and an `ownerAccountId` that differs
from its application-resource ARNs. The simplified
[producer envelope example](event-envelope.md#producer-obligations-the-event-envelope)
uses an object `Detail`, unlike the serialized `PutEvents` entry shown above it.
These examples were preserved, not promoted to executable fixtures.

The earlier style pass also left `409 s`, `400 s`, and `503 s` in some HTTP
failure prose. Review these as status-code plurals, not timing claims. No
runtime or latency meaning is inferred from them during this reorganization.

Resolution: the domain guide's response examples are valid JSON whose account
IDs match their ARNs. The producer envelope example serializes `Detail` as a
JSON string, like the `PutEvents` entry. The status-code plurals now read as
status codes (`fails with 409`, `503` responses, `rejected with 400`).

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

**Documentation aligned 2026-10-25:**
[resources and naming](resources.md#resource-namespace-convention) and
[platform boundaries](../architecture/platform-boundaries.md) no longer call
the trust role the only org-prefixed platform resource. They now name the
Cognito, system-events, and web console resources that also use
`{org}-konductor-*`. The migration question stays open.

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

**Documentation resolved 2026-10-25:** the uninstall procedure now empties the
web console bucket before deleting the core stack, deletes the guardrails
stack before the trust stacks, states that the DynamoDB tables go with the
stack, and lists the retained user pool and system-event schema registry with
cleanup commands. Cognito pool adoption remains an open design question.

**Documentation updated 2026-10-26:** the procedure also empties the
system-event projector failure bucket
(`konductor-system-event-failures-{account}-{region}`) before deleting the
core stack. The bucket keeps failed projector batches for 90 days, and
CloudFormation cannot delete it while it holds any.

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

**Documentation resolved 2026-10-25:** the
[recovery guide](../operations/domain-recovery.md#schemas-and-event-bus-rules-block-deprovisioning)
documents the manual purge (unsubscribe or delete leftover rules, targets
first, then delete each registry's schemas) before a teardown re-run, and the
uninstall procedure's domain step links to it. Whether deprovision-env should
purge schemas and rules itself stays open.

**Documentation updated 2026-10-26:** the purge also covers rules on the
domain's application bus. Konductor creates none there, but a domain team's
own integration rules make `DeleteAppEventBus` fail the same way, and
unsubscribing does not remove them. The recovery guide also removes the
domain's subscriptions in both directions before the first teardown run.
The teardown does not remove subscriptions, so a consumer's inbound
subscriptions would otherwise stay `ACTIVE`, with their rules, targets, and
alarms on the producers' platform buses, and a later subscribe request for
the same stream would be a no-op. Whether the teardown should remove
subscriptions itself belongs to the same open question.

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

**Documentation resolved 2026-10-25:**
[installation troubleshooting](../operations/installation-troubleshooting.md#resources-retained-after-stack-delete-or-a-failed-create)
explains rollback-retry cleanup (the retained registry, stranded user pools,
and the orphaned bootstrap log group) and points to the uninstall cleanup
commands. Registry adoption, automatic purge, and a fixed-name preflight stay
open.

## D27

**The domain registration field table omits `owningTeam`, which is
required for OAuth callers.** — **Resolved 2026-10-25.**

Original finding: `docs/guides/domains.md` documented the
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

Resolution: the [domain guide's field table](../guides/domains.md#fields) now
lists `owningTeam`. It is required on the OAuth surface, where it must name
one of the caller's team groups. The IAM surface does not accept it: its
request schema has no such field and API Gateway rejects the request with
`400`, so the "optional for IAM callers" suggestion above does not apply. A
domain registered through IAM gets no owner binding at approval, and the
guide says how a platform administrator grants one. The IAM example omits the
field and points to the OAuth `curl` example in the authentication guide.

## D28

**An invalid `costCenter` at registration became a system rejection.** —
**Resolved 2026-10-25.**

Original finding: a second review on 2026-10-25 found that the registration
handler validated `costCenter` inside the resource-name collision preflight.
A value that failed the governed-tag rule (empty, over 256 characters, or
containing control characters) was recorded as a `REJECTED` row with
`decidedBy: system`, and that row counted toward the
[domain ceiling](data-model.md#domain-count-ceiling), which bounds lifetime
registrations. The check depends only on the request, and the update command
refuses an invalid `costCenter` without writing. The
[domain-management spec](../../openspec/specs/domain-management/spec.md)
records system rejections only for resource-name collisions. The pages also
disagreed with each other: the
[domain guide](../guides/domains.md#what-happens-next) left `costCenter` out
of its registration outcomes, while
[domain lifecycle](../architecture/domain-lifecycle.md#registration-admission)
and [domain statuses](domain-statuses.md#domain-lifecycle) described it as a
platform-state check.

Resolution: the maintainer chose to change the handler rather than document
the row. The handler now checks `costCenter` with the other request-only
checks, before any write. An invalid value emits `RegisterDomainFailed`,
writes no record, charges no counter, and starts no workflow. Evidence: the
"Invalid cost center at registration is rejected" scenario in the
domain-management spec and
`invalid_cost_center_fails_before_any_write_or_workflow`
(domain-registrations). The domain guide, domain lifecycle, and domain
statuses list `costCenter` as a refusal, and `decidedBy: system` now comes
only from the collision check. Rows that earlier releases recorded this way
stay `REJECTED` and keep their ceiling slot. Re-registering the name replaces
such a row without a second charge.

## D29

**The wedged-lease procedure cancels the lease before it stops the
execution.** A prose pass over the published pages on 2026-10-25 found this
conflict. The
[schema promotion runbook](../operations/schema-promotion.md#releasing-a-wedged-promotion-lease)
says to cancel the lease manually, then stop the execution if one is still
running, and its two commands run in that order. The note after the commands
says to cancel only a promotion whose execution is confirmed stopped or
absent, because a cancelling-and-running pair can interleave replay writes.

`CANCELLED` is a terminal phase. The head-item put in the schema-promotions
handler (`put_promotion`) succeeds when the item is missing or its phase is
`COMPLETED`, `FAILED`, or `CANCELLED`. A cancelled lease therefore admits a
new promotion at once, and that promotion can replay while the old execution
is still running. Review whether the procedure should stop the execution,
confirm it is no longer running, and only then cancel the lease. The prose
pass kept the existing order.

## D30

**Subscription removal deletes zero-events alarms best effort.** A
pull-request review on 2026-10-26 found this conflict. The
[subscription architecture](../architecture/subscriptions.md#subscription-flow)
said unsubscribing removes targets, rules, and alarms in every producer
environment before the record becomes `REMOVED`. In
`remove-subscription.asl.yaml`, the `DeleteZeroEventsAlarm` step has no
`Retry` and catches `States.ALL`, so a failed alarm deletion is skipped and
the record still becomes `REMOVED`. A leftover alarm has no actions, but it
treats missing data as breaching, so it stays in the `ALARM` state.

**Documentation resolved 2026-10-26:** the architecture page describes the
alarm deletion as best effort, and the
[cleanup procedure](../operations/domain-recovery.md#cleaning-up-domain-resources)
checks every environment for a leftover alarm after the domain's
subscriptions show `REMOVED`. Whether the workflow should retry the deletion
or report a failed one stays open.
