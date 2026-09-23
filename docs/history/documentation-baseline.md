# Documentation baseline snapshots

[Documentation home](../README.md)

These excerpts come from commit `40bfdd5`, before the documentation reorganization. They are preserved as review evidence, not current instructions. See the [discrepancy register](../reference/documentation-discrepancies.md).

## Original problem statement

Cross-account routing with Amazon EventBridge requires resources in both producer and consumer accounts. These include event buses, rules, AWS Identity and Access Management (IAM) roles, resource policies, dead-letter queues (DLQs), and monitoring. A single domain integration involves the following work:

- A developer manages **12+ AWS resources** across two accounts
- Infrastructure setup delays the first event by **1–2 days**
- Each team reinvents the same resource topology with subtle drift
- Platform teams have no visibility into which domains exist or who owns them

Konductor for Amazon EventBridge eliminates this complexity. It gives platform teams a governance layer for domains and routing, while application teams get a simple API to register, publish, and subscribe to events without ever touching EventBridge resources directly.

## Previous architecture endpoint inventory

| Method | Path | Type | Handler |
|--------|------|------|---------|
| `POST` | `/domains` | Async (SQS) | `domain-registrations` (register) |
| `GET` | `/domains` | Sync (Lambda proxy) | `list-domains` |
| `GET` | `/domains/{domainName}` | Sync (Lambda proxy) | `describe-domain` |
| `PUT` | `/domains/{domainName}` | Async (SQS) | `domain-registrations` (update) |
| `POST` | `/domains/registrations/decide` | Sync (Lambda proxy) | `decide-registration` |
| `POST` | `/subscriptions` | Async (SQS) | `subscriptions` (subscribe) |
| `POST` | `/subscriptions/state` | Async (SQS) | `subscriptions` (enable/disable per env) |
| `GET` | `/subscriptions` | Sync (Lambda proxy) | `list-subscriptions` (direction-scoped) |
| `GET` | `/subscriptions/{subscriptionId}` | Sync (Lambda proxy) | `describe-subscription` |
| `DELETE` | `/subscriptions/{subscriptionId}` | Async (SQS) | `subscriptions` (unsubscribe) |
| `GET` | `/schemas` | Sync (Lambda proxy) | `list-schemas` (entitlement-expanded catalog) |
| `GET` | `/schemas/{schemaName}` | Sync (Lambda proxy) | `describe-schema` |
| `POST` | `/publish` | Sync admission + async delivery | `publish-events-admission` (validate-and-stamp) → `publish-events` (delivery) |
| `POST` | `/schemas` | Sync (Lambda proxy) | `schema-admin` (create schema, version 1) |
| `POST` | `/schemas/versions` | Sync (Lambda proxy) | `schema-admin` (append schema version) |
| `POST` | `/bindings` | Sync (Lambda proxy, OAuth only) | `binding-admin` (create) |
| `GET` | `/bindings` | Sync (Lambda proxy, OAuth only) | `binding-admin` (list by domain) |
| `DELETE` | `/bindings/{bindingId}` | Sync (Lambda proxy, OAuth only) | `binding-admin` (revoke) |
| `POST` | `/entitlements` | Sync (Lambda proxy, OAuth + IAM) | `entitlement-admin` (create) |
| `GET` | `/entitlements` | Sync (Lambda proxy, OAuth + IAM) | `entitlement-admin` (list) |
| `DELETE` | `/entitlements/{entitlementId}` | Sync (Lambda proxy, OAuth + IAM) | `entitlement-admin` (revoke) |

Each command handler Lambda receives SQS messages with a `HttpMethod` message attribute and dispatches accordingly (`domain-registrations`: register/update; `subscriptions`: subscribe/update-state/unsubscribe). Subscription commands carry the versioned trusted-principal envelope in message attributes; the handler reconstructs the principal and authorizes via `DynamoGrantEngine` before any state change.

## Previous OAuth endpoint inventory

The following table lists the OAuth endpoints and their authorization requirements.

| Method | Path | Required role | Description |
|--------|------|--------------|-------------|
| GET | `/domains` | admin or auditor (unscoped); team callers see owned domains | List domains (paginated, filterable) |
| GET | `/domains?view=directory` | any authenticated caller | Names-only directory projection (name, namespace, description, status) for pickers |
| POST | `/domains` | any authenticated caller with a team group | Register a new domain owned by `owningTeam` (async, 202) |
| GET | `/domains/{domainName}` | admin or auditor; owning team | Describe a single domain (404 for other teams, same as missing) |
| PUT | `/domains/{domainName}` | owning team or admin | Update domain metadata—description, emails, tags (async, 202) |
| GET | `/domains/registrations/pending` | admin | Approvals inbox: pending registrations with `taskToken` and owning team |
| POST | `/domains/registrations/decide` | admin | Approve or reject a registration |
| POST | `/domains/{domainName}/state` | admin | Enable/disable a domain (`{"action": "enable"\|"disable"}`); shallow: existing estate is retained |
| GET | `/subscriptions?domain=…&direction=…` | admin or auditor | List subscriptions (inbound/outbound, binding-scoped) |
| POST | `/subscriptions` | admin | Subscribe to a producer schema (async, 202) |
| POST | `/subscriptions/state` | admin | Enable/disable per environment (async, 202) |
| GET | `/subscriptions/{subscriptionId}` | admin or auditor | Describe a subscription |
| DELETE | `/subscriptions/{subscriptionId}` | admin | Unsubscribe (async, 202) |
| GET | `/schemas?consumerDomain=…` | admin or auditor; subscriber binding | Subscriber catalog (entitlement-expanded; omit consumerDomain for the platform-wide view) |
| GET | `/schemas?domain=…` | publisher binding on the domain | Producer scope: the publisher's own catalog without self-entitlements |
| GET | `/schemas/{schemaName}?consumerDomain=…&domain=…` | admin or auditor | Describe a schema (latest versions per env; 403 when uncovered) |
| GET | `/schemas/{schemaName}?domain=…` | publisher binding on the domain | Describe an own-domain schema without a consumer context |
| POST | `/bindings` | admin | Create a role binding (idempotent); `owner` bindings transfer domain ownership |
| GET | `/bindings?domain=…` | admin | List role bindings for a domain |
| DELETE | `/bindings/{bindingId}` | admin | Revoke a role binding (durable) |
| GET | `/schemas/{schemaName}/contract?domain=…&environment=…` | existing catalog authorization | Composed contract for the exact current pair, or an explicit historical pair (`schemaVersion` **and** `metadataRevision` together; 0 is the legacy selector; 404 for unknown pairs, 400 for half-selected pairs) |
| POST | `/schemas/contract-preview` | publisher binding on the domain | Compose unsaved business content against the environment's active metadata; registers nothing |
| GET | `/metadata/revisions` | platform roles | Immutable metadata revision history (newest first, paginated) |
| POST | `/metadata/revisions` | platform-admin | Create the next additive revision (stale head → 409; never activates) |
| POST | `/metadata/preview` | platform roles | Non-persisting synthetic preview of a draft definition (candidate revision, generated fields, bounds) |
| GET | `/metadata/environments` | platform roles | Active metadata pointer per environment (legacy flag until activated) |
| POST | `/metadata/readiness` | platform roles | Non-persisting readiness report for a candidate revision in an environment |
| GET | `/metadata/activations` | platform roles | Activation audit history (paginated) |
| POST | `/metadata/activations` | platform-admin | Activate a revision for one environment (expected-current condition; forward-only; stale state → 409) |
| POST | `/entitlements` | admin | Create a subscriber entitlement (deterministic key, idempotent) |
| GET | `/entitlements?consumerDomain=…` | admin | List entitlements by consumer or producer domain |
| DELETE | `/entitlements/{entitlementId}` | admin | Revoke an entitlement (converges subscriptions via the revocation pipeline) |

## Infrastructure as code

All infrastructure is defined across three CloudFormation templates. The core platform resources use AWS SAM (`konductor-core-stack.yaml`); SES email identities are managed by an optional sibling stack (`notifications-stack.yaml`); and per-domain-account trust roles use plain CloudFormation (`trust-stack.yaml`). Step Functions workflows are defined in YAML with JSONata query language.

### Project structure

```text
evb-konductor/
├── api/
│   └── openapi.yaml                          # OpenAPI 3.0 contract (embedded in SAM)
├── domain-management/
│   ├── functions/
│   │   ├── decide-registration/              # Sync: approve/reject decisions
│   │   ├── describe-domain/                  # Sync: get domain by ID
│   │   ├── domain-registrations/           # Async: register + update commands
│   │   ├── list-domains/                     # Sync: list/filter domains
│   │   ├── send-approval-request-notification/  # Approval request email (invoked by workflow)
│   │   ├── send-rejection-outcome-notification/ # Rejection outcome email (EventBridge rule)
│   │   └── send-provisioning-failed-notification/ # Provisioning failure email (EventBridge rule)
│   └── state-machines/
│       ├── approval.asl.yaml                 # Human-in-the-loop approval
│       ├── provision-domain.asl.yaml         # Outer coordinator (fan-out over environments)
│       ├── provision-env.asl.yaml            # Per-env resource creation + inner saga
│       └── deprovision-env.asl.yaml          # Idempotent per-env teardown
├── event-streams/
│   ├── functions/
│   │   ├── subscriptions/                    # Async: subscribe/state/unsubscribe commands
│   │   ├── materialize-subscription-env/     # Per-env materialization unit (workflow + extender invoke it)
│   │   ├── subscription-extender/            # Promotion-following extension on SchemaVersionPublished
│   │   ├── list-subscriptions/               # Sync: direction-scoped stream listing
│   │   ├── describe-subscription/            # Sync: stream detail by subscription id
│   │   ├── list-schemas/                     # Sync: entitlement-expanded catalog
│   │   └── describe-schema/                  # Sync: schema detail + content (coverage-checked)
│   └── state-machines/
│       ├── activate-subscription.asl.yaml    # Per-env materialization via the shared function, no saga
│       └── remove-subscription.asl.yaml      # Idempotent per-env rule removal (all producer envs)
├── lib/
│   └── konductor-common/                         # Shared library (models, DDB helpers, API, validation, namespace)
├── docs/
│   ├── development/
│   │   ├── architecture.md                   # This document
│   │   ├── decisions.md                      # Decision log
│   │   └── testing.md                        # Testing strategy
│   ├── domain-management.md                 # Domain owner guide (register, approve, operate)
│   └── trust-stack-deployment.md             # Domain account onboarding guide
├── konductor-core-stack.yaml                     # SAM template (core platform resources)
├── notifications-stack.yaml                  # Optional SES identity stack
├── trust-stack.yaml                          # CloudFormation for domain account trust role
├── samconfig-core.toml                       # SAM config for core stack
├── samconfig-notifications.toml              # SAM config for notifications stack
└── Cargo.toml                                # Rust workspace root
```

### Tech stack

| Layer | Technology |
|-------|-----------|
| Language | Rust (cargo-lambda, aws-sdk-rust, serde, tokio) |
| API | REST via API Gateway with separate OAuth (Cognito + REQUEST authorizer) and AWS_IAM surfaces |
| Database | DynamoDB (PAY_PER_REQUEST, single table) |
| Workflows | Step Functions (YAML, JSONata, SDK integrations) |
| Notifications | SES for owner emails (approval, rejection, provisioning, registration outcomes); SNS topic for operator lifecycle alerts (optional via notifications-stack.yaml) |
| Namespace sharing | SSM Advanced Parameters + RAM |
| IaC | AWS SAM / CloudFormation |
| Runtime | `provided.al2023` (ARM64, 256 MB) |
