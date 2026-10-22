# Decision index

[Documentation home](../README.md)

Use these records to understand why the platform took its present shape.
They preserve rationale, constraints, alternatives, failure analysis, and
stable change IDs. Read the relevant topic, not the entire history.

The migrated dates are the dates recorded in the old log. They can differ
from archive and merge dates. Records are not a fresh implementation audit.
Follow explicit supersession links and check [open discrepancies](../reference/documentation-discrepancies.md)
before treating an older claim as a current requirement.

## Domains, provisioning, and names

- [Per-environment account overrides for domain provisioning](per-environment-domain-accounts.md)
- [Unified domain lifecycle and workflow-owned outcomes](domain-management-redesign.md)
- [Namespace convention, compensation, account detection, and sharing](konductor-provisioning-enhancements.md)
- [Provisioning outcome event names](provisioning-event-cleanup.md)
- [Optional notification stack](extract-notifications-stack.md)
- [Multi-environment provisioning and rollback](multi-env-domain-provisioning.md)
- [Singleton platform and fixed resource names](konductor-platform-drop-environment.md)
- [Domain name as the primary key](domain-name-as-primary-key.md)
- [Workflow failure paths and retries](harden-workflow-failure-paths.md)
- [Domain-count ceiling and atomic counter](cap-domain-count.md)
- [Product and repository rename](rename-to-konductor.md)

## Identity and access

- [Authorization-engine evaluation and accepted uncertainty](evaluate-authorization-engine.md)
- [Cognito identity, trusted principal, and dual surfaces](add-cognito-access-foundation.md)
- [Production grant engine and bindings](add-grant-engine.md)
- [Subscriber entitlements and revocation](add-subscriber-entitlements.md)

## Event contracts and delivery

- [Schema-first subscriptions and version pinning](add-event-subscriptions.md)
- [Publisher APIs and environment-named stages](add-publisher-apis.md)
- [Validate at ingest, not at delivery](validate-at-ingest.md)
- [Independent per-environment subscription materialization](follow-schema-promotion.md)
- [Replay-based schema promotion with aligned version numbers](promote-schema-versions.md)
- [Fail-closed schema compatibility proof](harden-schema-compatibility-checker.md)
- [Public system events for subscription upgrades](domain-system-events.md)
- [Independent platform metadata contracts](add-platform-metadata-composition.md)

## Trust and operations

- [Platform configuration contract and preflight](configure-customer-platform.md)
- [Trust-stack deployment targets](add-trust-stack-deploy.md)
- [Organization guardrails and ownership-aware console links](protect-domain-managed-resources.md)
- [Single installation entry point](installation-guide.md)

## Contributors and the console

- [Initial test foundation](unit-test-foundation.md)
- [CI templates and original Gitflow decision](github-ci-templates.md)
- [Trunk-based branching, superseding Gitflow](trunk-based-branching.md)
- [SDK test features scoped to development dependencies](sdk-test-util-scoping.md)
- [Schema authoring in the console](schema-authoring-ui.md)
- [Reka primitives and local notification lifetime](refine-ui-with-reka.md)
- [Controlled browser time in UI tests](stabilize-ui-test-timing.md)
- [Documentation organization and maintenance](documentation-organization.md)

## Durability and unresolved work

- [Side-effects audit decision](audit-stream-driven-side-effects.md)
- [Full per-handler classifications and failure walks](../development/side-effects/README.md)
- [Recorded deferrals and follow-up IDs](deferred.md)
- [Documentation discrepancies awaiting separate review](../reference/documentation-discrepancies.md)

For what changed recently, read the [changelog](../../CHANGELOG.md).
For new records, follow [documentation maintenance](../development/documentation.md#record-decisions-without-recreating-a-log).
