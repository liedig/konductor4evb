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

Review the Cognito client configuration and supported login flows. User setup
also states a 12-character minimum but includes a shorter temporary-password
example. These examples were moved unchanged, not tested against a live pool.

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

