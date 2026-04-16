# EventBridge Nexus

A serverless integration platform for managing AWS EventBridge infrastructure across accounts. It provides domain-level APIs that let teams publish and subscribe to events without manually creating EventBridge rules, IAM policies, or cross-account delivery infrastructure.

## Why

Cross-account EventBridge setups are complex. Each subscription typically requires coordinating event buses, rules, resource policies, IAM roles, and DLQs across multiple AWS accounts. EventBridge Nexus abstracts that away behind a REST API, reducing time-to-first-event from days to minutes and cutting the number of resources teams need to manage per subscription by roughly 60%.

## Key Concepts

- **Domain** -- A logical grouping of related events owned by a team. Each domain gets its own event buses, schema registry, DLQs, and CloudWatch monitoring, all provisioned automatically.
- **Platform account** -- The central account where the Nexus runs. Domain event buses live here with policies restricting who can publish.
- **Application account** -- A team's AWS account. Each gets a trust stack that grants the Nexus cross-account access to create domain resources.
- **Namespace convention** -- All resources follow a configurable `{org}-{env}-{domain}` naming pattern (e.g., `acme-prod-payments-domain-bus`).

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
├── template.yaml                     # SAM template
├── trust-stack.yaml                  # App account CloudFormation
├── samconfig.toml
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

### Prerequisites

- Rust toolchain with [cargo-lambda](https://www.cargo-lambda.info/)
- [AWS SAM CLI](https://docs.aws.amazon.com/serverless-application-model/latest/developerguide/install-sam-cli.html)
- AWS credentials configured for the platform account

### Build and Test

```bash
make check    # cargo check + clippy
make test     # cargo test --workspace
make build    # sam build
```

### Deploy

1. Deploy the Nexus to the platform account:

```bash
sam deploy --guided
```

2. Deploy the trust stack in each application account:

```bash
aws cloudformation deploy \
  --template-file trust-stack.yaml \
  --stack-name <org>-<env>-nexus-trust-stack \
  --parameter-overrides \
    NexusRoleArn=<function-role-arn> \
    ProvisioningWorkflowRoleArn=<provisioning-role-arn> \
    Environment=<env> \
    Organization=<org> \
  --capabilities CAPABILITY_NAMED_IAM
```

See [`docs/trust-stack-deployment.md`](docs/trust-stack-deployment.md) for detailed instructions.

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
