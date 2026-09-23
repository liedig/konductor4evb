# Documentation moved

Use [README](../architecture/README.md). This page preserves links from older changes and bookmarks; do not add new guidance here.

## Previous sections

| Previous section | Current location |
|---|---|
| <a id="konductor-for-amazon-eventbridge-architecture"></a>Konductor for Amazon EventBridge architecture | [Open section](../architecture/README.md) |
| <a id="the-problem"></a>The problem | [Open section](../architecture/README.md#the-problem) |
| <a id="system-overview"></a>System overview | [Open section](../architecture/platform-boundaries.md#system-overview) |
| <a id="key-design-decisions"></a>Key design decisions | [Open section](../architecture/platform-boundaries.md#key-design-decisions) |
| <a id="account-per-domain-ownership-model"></a>Account-per-domain ownership model | [Open section](../architecture/domain-lifecycle.md#account-per-domain-ownership-model) |
| <a id="domain-lifecycle"></a>Domain lifecycle | [Open section](../reference/domain-statuses.md#domain-lifecycle) |
| <a id="api-layer"></a>API layer | [Open section](../architecture/authorization.md#api-layer) |
| <a id="identity-and-authorization"></a>Identity and authorization | [Open section](../architecture/authorization.md#identity-and-authorization) |
| <a id="grant-engine"></a>Grant engine | [Open section](../architecture/authorization.md#grant-engine) |
| <a id="entitlements"></a>Entitlements | [Open section](../architecture/entitlements.md#entitlements) |
| <a id="endpoints"></a>Endpoints | [Open section](../reference/api.md) |
| <a id="approval-workflow"></a>Approval workflow | [Open section](../architecture/domain-lifecycle.md#approval-workflow) |
| <a id="provisioning-workflow"></a>Provisioning workflow | [Open section](../architecture/provisioning.md#provisioning-workflow) |
| <a id="outer-workflow-provision-domain"></a>Outer workflow (provision-domain) | [Open section](../architecture/provisioning.md#outer-workflow-provision-domain) |
| <a id="inner-workflow-provision-env"></a>Inner workflow (provision-env) | [Open section](../architecture/provisioning.md#inner-workflow-provision-env) |
| <a id="resources-created-per-domain"></a>Resources created per domain | [Open section](../reference/resources.md#provisioned-resources) |
| <a id="saga-compensation-rollback"></a>Saga compensation (rollback) | [Open section](../architecture/provisioning.md#saga-compensation-rollback) |
| <a id="cross-account-trust-model"></a>Cross-account trust model | [Open section](../architecture/cross-account-trust.md#cross-account-trust-model) |
| <a id="trust-role-permissions"></a>Trust role permissions | [Open section](../architecture/cross-account-trust.md#trust-role-permissions) |
| <a id="subscription-delivery-authorization"></a>Subscription delivery authorization | [Open section](../architecture/cross-account-trust.md#subscription-delivery-authorization) |
| <a id="onboarding-a-new-domain-account"></a>Onboarding a new domain account | [Open section](../operations/trust-stack-deployment.md) |
| <a id="organization-guardrails-scp-ceiling"></a>Organization guardrails (SCP ceiling) | [Open section](../operations/guardrails.md) |
| <a id="resource-namespace-convention"></a>Resource namespace convention | [Open section](../reference/resources.md#resource-namespace-convention) |
| <a id="domain-namespace-environment-independent"></a>Domain namespace (environment-independent) | [Open section](../reference/resources.md#domain-namespace-environment-independent) |
| <a id="event-bus-and-lifecycle-events"></a>Event bus and lifecycle events | [Open section](../reference/lifecycle-events.md#event-bus-and-lifecycle-events) |
| <a id="event-catalog"></a>Event catalog | [Open section](../reference/lifecycle-events.md#event-catalog) |
| <a id="event-subscriptions-schema-first"></a>Event subscriptions (schema-first) | [Open section](../architecture/subscriptions.md#event-subscriptions-schema-first) |
| <a id="the-canonical-event-envelope"></a>The canonical event envelope | [Open section](../reference/event-envelope.md#the-canonical-event-envelope) |
| <a id="event-publishing-validate-at-ingest"></a>Event publishing (validate-at-ingest) | [Open section](../architecture/publish-validation.md#3-the-two-halves-admission-and-delivery) |
| <a id="schema-management-append-only"></a>Schema management (append-only) | [Open section](../architecture/schemas.md#schema-management-append-only) |
| <a id="platform-metadata-composition"></a>Platform metadata composition | [Open section](../architecture/metadata.md#platform-metadata-composition) |
| <a id="subscription-materialization"></a>Subscription materialization | [Open section](../architecture/subscriptions.md#subscription-materialization) |
| <a id="subscription-flow"></a>Subscription flow | [Open section](../architecture/subscriptions.md#subscription-flow) |
| <a id="subscription-lifecycle"></a>Subscription lifecycle | [Open section](../architecture/subscriptions.md#subscription-lifecycle) |
| <a id="schema-discovery-subscriber-catalog"></a>Schema discovery (subscriber catalog) | [Open section](../architecture/subscriptions.md#schema-discovery-subscriber-catalog) |
| <a id="data-model"></a>Data model | [Open section](../reference/data-model.md) |
| <a id="table-schema"></a>Table schema | [Open section](../reference/data-model.md#table-schema) |
| <a id="access-patterns"></a>Access patterns | [Open section](../reference/data-model.md#access-patterns) |
| <a id="domain-count-ceiling"></a>Domain-count ceiling | [Open section](../reference/data-model.md#domain-count-ceiling) |
| <a id="event-streams-table"></a>Event streams table | [Open section](../reference/data-model.md#event-streams-table) |
| <a id="infrastructure-as-code"></a>Infrastructure as code | [Open section](repository.md) |
| <a id="project-structure"></a>Project structure | [Open section](repository.md) |
| <a id="tech-stack"></a>Tech stack | [Open section](repository.md) |
| <a id="cross-references"></a>Cross-references | [Open section](../README.md) |
