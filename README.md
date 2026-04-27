# EventBridge Nexus

A serverless integration platform for managing AWS EventBridge infrastructure across accounts. It provides domain-level APIs that let teams publish and subscribe to events without manually creating EventBridge rules, IAM policies, or cross-account delivery infrastructure.

## Why

Cross-account EventBridge setups are complex. Each subscription typically requires coordinating event buses, rules, resource policies, IAM roles, and DLQs across multiple AWS accounts. EventBridge Nexus abstracts that away behind a REST API, reducing time-to-first-event from days to minutes and cutting the number of resources teams need to manage per subscription by roughly 60%.

## Key Concepts

- **Domain** -- A logical grouping of related events owned by a team. Each domain gets its own event buses, schema registry, DLQs, and CloudWatch monitoring, all provisioned automatically.
- **Platform account** -- The central account where the Nexus runs. Domain event buses live here with policies restricting who can publish.
- **Application account** -- A team's AWS account. Each gets a trust stack that grants the Nexus cross-account access to create domain resources.
- **Namespace convention** -- Platform resources follow the fixed `nexus-*` pattern (e.g., `nexus-bus`); the app-account trust role is `{org}-nexus-trust-role`; domain resources follow a configurable `{org}-{env}-{domain}-*` pattern (e.g., `acme-prod-payments-domain-bus`) and are provisioned per environment from the platform-configured `Environments` list.

## Architecture

```
                           Platform Account
                    ┌──────────────────────────────┐
                    │  API Gateway (IAM auth)       │
                    │       │                       │
                    │  Lambda Functions (Rust)       │
                    │       │                       │
                    │  DynamoDB    EventBridge       │
                    │  (domains,   (Nexus            │
                    │   requests)   lifecycle bus)   │
                    │       │                       │
                    │  Step Functions                │
                    │  ├─ Approval (Task Token)      │
                    │  ├─ Provisioning (SDK integ.)  │
                    │  └─ Cleanup                    │
                    │       │                       │
                    │  Domain Event Buses            │
                    │  Schema Registries             │
                    │  Subscription DLQs             │
                    │  CloudWatch Dashboards/Alarms  │
                    └──────────┬───────────────────┘
                               │ STS AssumeRole
                    ┌──────────▼───────────────────┐
                    │      Application Account      │
                    │  Trust Role (nexus-trust-role)  │
                    │  Domain Event Bus (app-side)   │
                    │  Integration DLQ               │
                    └──────────────────────────────┘
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

```
.
├── api/
│   └── openapi.yaml                  # API contract
├── domain-management/
│   ├── functions/                    # Lambda handlers
│   │   ├── register-domain/
│   │   ├── list-domains/
│   │   ├── describe-domain/
│   │   ├── update-domain/
│   │   ├── delete-domain/
│   │   ├── approve-registration/
│   │   └── list-pending-activations/
│   └── state-machines/               # Step Functions ASL
│       ├── approval.asl.json
│       ├── provisioning.asl.json
│       └── cleanup.asl.json
├── event-routing/                    # Event routing (planned)
│   └── functions/
├── lib/
│   └── nexus-common/                 # Shared library
│       └── src/
│           ├── api.rs                # API Gateway helpers
│           ├── dynamodb.rs           # DynamoDB helpers
│           ├── error.rs              # Error types
│           ├── models.rs             # Domain models
│           └── namespace.rs          # Resource naming
├── docs/
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

All endpoints require IAM authentication.

| Method | Path | Description |
|---|---|---|
| `POST` | `/domains` | Register a new domain |
| `GET` | `/domains` | List domains (filter by owner, status) |
| `GET` | `/domains/{domainId}` | Describe a domain |
| `PUT` | `/domains/{domainId}` | Update domain metadata |
| `DELETE` | `/domains/{domainId}` | Delete a domain |
| `GET` | `/domains/activations` | List pending registrations |
| `POST` | `/domains/activations/{requestId}/decision` | Approve or reject a registration |

Full API contract: [`api/openapi.yaml`](api/openapi.yaml)

## Domain Lifecycle

```
RegisterDomain ──> PENDING_APPROVAL
                        │
        Approve ────────┼────── Reject
           │                      │
      APPROVED                REJECTED
           │
   Provisioning Workflow
   (bus, schema registry,
    DLQs, dashboard, alarms
    across both accounts)
           │
        ACTIVE
        │    │
   Suspend  Delete
      │       │
  SUSPENDED  DELETING
      │       │
   Resume    Cleanup Workflow
      │       │
   ACTIVE   DELETED
```

## Getting Started

See [docs/installation.md](docs/installation.md) for the complete installation guide covering prerequisites, three-stack deployment, smoke test, troubleshooting, and uninstall.

## Workflows

### Approval

Human-in-the-loop registration approval using the Step Functions Task Token pattern. The workflow waits indefinitely for a platform team member to approve or reject via the API.

### Provisioning

Triggered automatically when a domain is approved. Uses Step Functions SDK integrations (no intermediate Lambdas) to create resources across both accounts:

1. Create domain event bus in platform account with PutEvent policy
2. Create schema registry
3. Create subscription DLQ (platform account)
4. Assume role in application account
5. Create domain event bus (app account)
6. Create integration DLQ (app account)
7. Create CloudWatch dashboard and alarms
8. Update domain record to ACTIVE

### Cleanup

Triggered by DomainDeletionRequested event. Tears down all provisioned resources in both accounts in parallel where possible, then marks the domain as DELETED.

## Resource Topology Per Domain

| Resource | Account | Name Pattern |
|---|---|---|
| Domain event bus | Platform | `{org}-{env}-{domain}-domain-bus` |
| Schema registry | Platform | `{org}-{env}-{domain}-schema-registry` |
| Subscription DLQ | Platform | `{org}-{env}-{domain}-subscription-dlq` |
| CloudWatch dashboard | Platform | `{org}-{env}-{domain}-domain-dashboard` |
| Domain event bus | Application | `{org}-{env}-{domain}-domain-bus` |
| Integration DLQ | Application | `{org}-{env}-{domain}-integration-dlq` |
