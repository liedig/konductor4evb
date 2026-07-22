# EventBridge Nexus

A serverless platform for governing cross-account EventBridge at scale. Register domains, discover schemas, and subscribe to events without touching IAM policies, bus policies, or cross-account delivery infrastructure.

[Installation guide](docs/installation.md) | [Architecture](docs/development/architecture.md) | [API contract](api/openapi.yaml)

## Why

Cross-account EventBridge is powerful but operationally complex. Each subscription typically requires coordinating event buses, rules, resource policies, IAM roles, and DLQs across two AWS accounts. Nexus abstracts that so teams can focus on their events, not their infrastructure.

**Governed domains.** Every domain gets event buses, schema registries, DLQs, and CloudWatch monitoring, provisioned automatically across all environments with saga rollback on failure.

**Schema-first subscriptions.** Consumers subscribe to a schema contract, not a bus. Nexus pins the latest version per environment, materializes EventBridge rules, and wires cross-account delivery with DLQs and zero-events alarms.

**Built-in ownership and observability.** Each domain has a primary owner and required secondary owners. Approval workflows, lifecycle events, CloudWatch dashboards, and per-subscription alarms come standard.

## How It Works

```text
  Platform Account                         Domain Account
  ┌──────────────────────┐                 ┌──────────────────┐
  │  API Gateway          │                │  Trust role       │
  │  Lambda (Rust)        │── STS ────────▶│  ({org}-nexus-   │
  │  DynamoDB             │   AssumeRole   │   trust-role)     │
  │  Step Functions       │                │                   │
  │  EventBridge bus      │                │  Domain bus       │
  │  Per-domain resources │                │  Integration DLQ  │
  └──────────────────────┘                 └──────────────────┘
```

Nexus runs in a dedicated platform account. Domain resources are provisioned into domain accounts via cross-account IAM roles. All control-plane state lives in the platform account; domain accounts only contain the resources their domains need.

## Core Capabilities

* **Domain lifecycle.** Register a domain via API, get human approval via email or API, and Nexus provisions all resources across every configured environment with all-or-nothing saga rollback. See [Domain Management Guide](docs/domain-management.md).

* **Schema discovery.** Browse a producer's schemas and see version skew across environments before subscribing. Schema registration stays out-of-band (via the AWS Schemas API) until a future schema-management capability lands.

* **Cross-account subscriptions.** Subscribe a consumer domain to a producer's schema. Nexus creates EventBridge rules, wires delivery to the consumer's domain bus, and sets up DLQs and zero-events alarms. Per-environment enable/disable maps to native rule state. See [Domain Management Guide](docs/domain-management.md#event-subscriptions).

* **Multi-environment reliability.** Every domain is provisioned across the platform-configured environment list (default: dev, staging, prod). Provisioning is all-or-nothing; deprovisioning is idempotent. See [Architecture](docs/development/architecture.md).

## Get Started

1. Deploy Nexus into your platform account — see the [Installation Guide](docs/installation.md)
2. Onboard each domain account with a trust stack — see [Trust Stack Deployment](docs/trust-stack-deployment.md)
3. Register and approve your first domain — see [Domain Management Guide](docs/domain-management.md)

Default AWS quotas cap a fresh platform account at **3 domains**. See [Service Quotas](docs/service-quotas.md) before onboarding more.

## Documentation

| Audience | Start here |
|---|---|
| Platform operators | [Installation Guide](docs/installation.md) |
| Domain owners / producers | [Domain Management Guide](docs/domain-management.md) |
| Contributors / maintainers | [Architecture](docs/development/architecture.md), [Testing Guide](docs/development/testing.md), [AGENTS.md](AGENTS.md) |

## Current Scope

- **No Publish API.** Producers publish directly to their platform-side domain bus and must stamp the [canonical event envelope](docs/development/architecture.md#the-canonical-event-envelope). A Publish API is planned.
- **Schema writes are out-of-band.** Domain owners register schemas via the AWS Schemas API. Schema management within Nexus is a future capability.
- **No domain deletion API.** Domain teardown is handled by `make teardown-domain`, which drives the same deprovisioning workflow used by the provisioning saga's rollback path. A `DELETE /domains` flow is planned.

## Technology

Rust Lambda functions on ARM64 Amazon Linux 2023, AWS SAM infrastructure, DynamoDB, Step Functions with JSONata, and EventBridge.
