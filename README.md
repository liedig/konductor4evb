# EventBridge Nexus

A serverless integration platform for managing AWS EventBridge infrastructure across accounts. It provides domain-level APIs that let teams publish and subscribe to events without manually creating EventBridge rules, IAM policies, or cross-account delivery infrastructure.

## Why

Cross-account EventBridge setups are complex. Each subscription typically requires coordinating event buses, rules, resource policies, IAM roles, and DLQs across multiple AWS accounts. EventBridge Nexus abstracts that away behind a REST API, reducing time-to-first-event from days to minutes and cutting the number of resources teams need to manage per subscription by roughly 60%.

## Key Concepts

- **Domain** -- A logical grouping of related events owned by a team. Each domain gets its own event buses, schema registry, DLQs, and CloudWatch monitoring, all provisioned automatically. Each domain has one primary owner and at least one secondary owner (backup contacts for when the primary owner is unavailable).
- **Platform account** -- The central account where the Nexus runs. Domain event buses live here with policies restricting who can publish.
- **Application account** -- A team's AWS account. Each gets a trust stack that grants the Nexus cross-account access to create domain resources.
- **Namespace convention** -- Platform resources follow the fixed `nexus-*` pattern (e.g., `nexus-bus`); the app-account trust role is `{org}-nexus-trust-role`; domain resources follow a configurable `{org}-{env}-{domain}-*` pattern (e.g., `acme-prod-payments-event-bus`) and are provisioned per environment from the platform-configured `Environments` list.

## Architecture

```text
                           Platform Account
                    ┌───────────────────────────────┐
                    │  API Gateway (IAM + email-link) │
                    │       │                         │
                    │  Lambda Functions (Rust)        │
                    │       │                         │
                    │  DynamoDB     EventBridge       │
                    │  (nexus-      (nexus-bus:       │
                    │   domains)     lifecycle)       │
                    │       │                         │
                    │  Step Functions                 │
                    │  ├─ approval (task token)       │
                    │  ├─ provision-domain (outer)    │
                    │  ├─ provision-env (per-env saga)│
                    │  └─ deprovision-env (teardown)  │
                    │       │                         │
                    │  Per-env domain resources:      │
                    │  ├─ Inbound event bus           │
                    │  ├─ Schema registry             │
                    │  ├─ Subscription DLQ            │
                    │  └─ Dashboard + alarms          │
                    └──────────┬──────────────────────┘
                               │ STS AssumeRole
                    ┌──────────▼──────────────────────┐
                    │       Application Account        │
                    │  Trust role ({org}-nexus-trust-  │
                    │    role, serves all envs)        │
                    │  Per-env domain resources:       │
                    │  ├─ Domain event bus             │
                    │  └─ Integration DLQ              │
                    └──────────────────────────────────┘
```

## Tech Stack

| Layer | Technology |
|---|---|
| Language | Rust |
| Compute | AWS Lambda (cargo-lambda, arm64) |
| API | API Gateway REST + OpenAPI 3.0 |
| Database | Amazon DynamoDB (table-per-entity) |
| Workflows | AWS Step Functions (SDK integrations) |
| Events | Amazon EventBridge |
| IaC | AWS SAM |

## Project Structure

```text
.
├── api/
│   └── openapi.yaml                  # API contract
├── domain-management/
│   ├── functions/                    # Lambda handlers
│   │   ├── domain-registrations/     # Async writes: register + update (SQS-triggered)
│   │   ├── list-domains/             # GET /domains
│   │   ├── describe-domain/          # GET /domains/{domainName}
│   │   ├── decide-registration/      # POST/GET /domains/registrations/decide
│   │   ├── send-approval-request-notification/    # Invoked by workflow (platform operator email)
│   │   ├── send-approval-outcome-notification/    # EventBridge-triggered (domain owner)
│   │   ├── send-rejection-outcome-notification/   # EventBridge-triggered (domain owner)
│   │   └── send-provisioning-failed-notification/ # EventBridge-triggered (domain owner)
│   └── state-machines/               # Step Functions ASL (YAML + JSONata)
│       ├── approval.asl.yaml         # Human-in-the-loop registration approval
│       ├── provision-domain.asl.yaml # Outer coordinator (fans out over envs)
│       ├── provision-env.asl.yaml    # Per-env resource creation + inner saga
│       └── deprovision-env.asl.yaml  # Idempotent per-env teardown
├── lib/
│   └── nexus-common/                 # Shared library
│       └── src/
│           ├── api.rs                # API Gateway helpers
│           ├── dynamodb.rs           # DynamoDB helpers
│           ├── error.rs              # Error types
│           ├── html.rs               # HTML response helpers
│           ├── models.rs             # Domain models
│           └── namespace.rs          # Resource naming
├── docs/
│   ├── domain-management.md         # Domain owner guide
│   └── trust-stack-deployment.md     # App account onboarding guide
├── nexus-core-stack.yaml             # SAM template (core platform)
├── notifications-stack.yaml          # SES identities (optional)
├── trust-stack.yaml                  # App account CloudFormation
├── samconfig-core.toml               # SAM config for core stack
├── samconfig-notifications.toml      # SAM config for notifications stack
├── Makefile
└── Cargo.toml                        # Workspace root
```

## API

All endpoints require IAM authentication **except** the `GET` email-link decide endpoint, which is unauthenticated — the Step Functions task token in the query string is the bearer credential.

| Method | Path | Auth | Description |
|---|---|---|---|
| `POST` | `/domains` | IAM | Register a new domain (async via SQS) |
| `GET` | `/domains` | IAM | List domains (filter by owner, status) |
| `GET` | `/domains/{domainName}` | IAM | Describe a domain (includes per-env `resourceArns` map) |
| `PUT` | `/domains/{domainName}` | IAM | Update domain metadata (async via SQS) |
| `POST` | `/domains/registrations/decide` | IAM | Approve or reject a registration (JSON body with task token) |
| `GET` | `/domains/registrations/decide?decision={approve\|reject}&token={taskToken}` | none | One-click approve/reject via email link (HTML confirmation page) |

Full API contract: [`api/openapi.yaml`](api/openapi.yaml)

## Domain Lifecycle

```text
RegisterDomain ──> PENDING_APPROVAL
                        │
        Approve ────────┼────── Reject
           │                      │
      PROVISIONING              REJECTED
           │
 provision-domain workflow
 (fans out over environments,
  saga rollback on any failure)
        │        │
     ACTIVE  PROVISIONING_FAILED
```

## Getting Started

See [docs/installation.md](docs/installation.md) for the complete installation guide covering prerequisites, three-stack deployment, smoke test, troubleshooting, and uninstall.

For domain owners, see [docs/domain-management.md](docs/domain-management.md) for the guide to registering, approving, and operating domains.

## Workflows

### Approval (`approval.asl.yaml`)

Human-in-the-loop registration approval using the Step Functions task-token pattern. The workflow waits up to 7 days for approval via the authenticated POST API or the one-click GET email link. On approve it emits `DomainRegistrationApproved` (which triggers provisioning). On reject (or timeout) it emits `DomainRegistrationRejected`.

### Provisioning (three-workflow split)

Triggered automatically by `DomainRegistrationApproved`. All-or-nothing across the platform-configured `Environments` list.

- **`provision-domain.asl.yaml`** (outer coordinator) — sets domain status to `PROVISIONING`, creates the env-independent namespace SSM parameter and RAM share, then fans out across the domain's environments using a Map state running `provision-env.sync:2` per entry. On all-success: updates status to `ACTIVE` with a per-env `resourceArns` map and emits `DomainProvisioningSucceeded`. On any-env-failure: fans out `deprovision-env.sync:2` over the envs that had succeeded, updates status to `PROVISIONING_FAILED`, and emits `DomainProvisioningFailed` with `failedEnvironment`, `failedStep`, `errorCode`, `errorMessage`, and aggregated `compensationErrors`.
- **`provision-env.asl.yaml`** (inner, per-env) — creates the platform-side domain bus, schema registry, subscription DLQ, dashboard, and alarms; assumes the trust role in the app account to create the matching app-side domain bus and integration DLQ. The platform and app buses share the same name — they live in different accounts, so there's no collision — which simplifies the mental model. Inner saga rolls back any resources it created if a later step fails.
- **`deprovision-env.asl.yaml`** (reusable teardown) — idempotent delete-everything-in-this-env workflow. Used for rollback today; the primitive for a future `DELETE /domains` flow.

## Resource Topology Per Domain

Domain resources are provisioned **once per environment** in the `Environments` list (default `dev,staging,prod`). Platform resources (bus, table, API, etc.) are singletons with the fixed `nexus-*` pattern — not listed here.

| Resource | Account | Name Pattern |
|---|---|---|
| Domain event bus | Platform | `{org}-{env}-{domain}-event-bus` |
| Schema registry | Platform | `{org}-{env}-{domain}-schema-registry` |
| Subscription DLQ | Platform | `{org}-{env}-{domain}-subscription-dlq` |
| CloudWatch dashboard | Platform | `{org}-{env}-{domain}-dashboard` |
| Failed-invocations alarm | Platform | `{org}-{env}-{domain}-failed-invocations-alarm` |
| DLQ-depth alarm | Platform | `{org}-{env}-{domain}-dlq-depth-alarm` |
| Domain event bus | Application | `{org}-{env}-{domain}-event-bus` |
| Integration DLQ | Application | `{org}-{env}-{domain}-integration-dlq` |

The platform-side and app-side domain buses intentionally share the same name — they're in different accounts, so there's no collision. Tip: to smoke-test end-to-end in a single AWS account (where platform = app), register each domain under a unique `domainName` per run so the test doesn't collide with its own previous attempt.

A single trust role (`{org}-nexus-trust-role`) in each application account serves all environments.
