# EventBridge Nexus

A serverless control plane for cross-account event-driven architecture at scale. Platform teams get governed domains — registration, approval, provisioning, ownership, and observability. Application teams get three simple verbs — **publish, subscribe, unsubscribe** — and never touch event buses, IAM policies, or cross-account delivery infrastructure.

[Installation guide](docs/installation.md) | [Architecture](docs/development/architecture.md) | [API contracts](api/) (OAuth: `openapi-oauth.yaml`, IAM: `openapi-iam.yaml`)

## Why Nexus

Consuming an event from another AWS account means coordinating event buses, rules, resource policies, IAM roles, and DLQs across two accounts — 12+ resources and days of infrastructure wrangling per integration, reinvented by every team with subtle drift. Nexus abstracts that away so teams focus on their events, not their plumbing.

**Minutes, not days.** Subscribe to an event and start processing it in minutes. Cross-account delivery is invisible.

**Governed domains.** Every domain gets event buses, schema registries, DLQs, and CloudWatch monitoring, provisioned automatically across all environments with all-or-nothing saga rollback. Human approval gates registration.

**Simple verbs, real durability.** Publishing is one `POST /publish` call. The event is durably queued before anything downstream can lose it, and retries preserve its idempotency key and original publish timestamp.

**Schema-first contracts.** Consumers subscribe to a schema contract, not a bus. Publishers manage schemas through the API — append-only and versioned per environment. Nexus pins the latest version and materializes the delivery rules.

**Built-in ownership and observability.** Every domain has a primary owner and required secondary owners. Approval workflows, lifecycle events, CloudWatch dashboards, and per-subscription alarms come standard.

## How It Works

```text
┌─ GOVERN ─────────────────────────────────────────────────────────────────┐
│  Register → human approval → every environment provisioned atomically:   │
│  buses, registries, DLQs, monitoring. The platform grants entitlements   │
│  that decide which consumer domains can see and subscribe to a schema.   │
├─ PUBLISH ────────────────────────────────────────────────────────────────┤
│  Manage schemas append-only (POST /schemas, POST /schemas/versions) and  │
│  publish with one call (POST /publish): publisher authorized, latest     │
│  version pinned, canonical envelope stamped, durably queued.             │
├─ SUBSCRIBE ──────────────────────────────────────────────────────────────┤
│  GET /schemas returns only what your domain's entitlements cover →       │
│  POST /subscriptions checks your binding and coverage → per-environment  │
│  rules wired across accounts → events arrive on your own bus.            │
├─ OPERATE ────────────────────────────────────────────────────────────────┤
│  Pause or resume a stream per environment. Revoke an entitlement and     │
│  uncovered streams disable automatically. Grants, revocations, and       │
│  command outcomes are audited; silent streams trip per-stream alarms.    │
└──────────────────────────────────────────────────────────────────────────┘
```

Nexus runs in a dedicated platform account. Domain resources are provisioned into domain accounts via cross-account IAM roles. All control-plane state lives in the platform account; domain accounts only contain the resources their domains need.

Both API surfaces serve **environment-named stages** (`dev`, `staging`, `prod`): the stage in the URL selects the target environment, so callers never name an environment, registry, or bus in a request.

## Core Capabilities

* **Domain lifecycle.** Register a domain via API, get human approval via email or API, and Nexus provisions all resources across every configured environment with all-or-nothing saga rollback. See [Domain Management Guide](docs/domain-management.md).

* **Event publishing.** `POST /publish` with a domain, event name, and business data — Nexus authorizes the publisher, resolves the latest schema version, stamps the canonical envelope, and delivers to the domain's event bus. Store-and-forward through SQS: once accepted (`202`), the event survives downstream failures and is retried with its idempotency key intact. See [Publishing Events](docs/publishing-events.md).

* **Schema management.** Publishers create schemas and append versions with `POST /schemas` and `POST /schemas/versions` — append-only, in platform-managed registries, targeting the environment selected by the API stage. See [Publishing Events](docs/publishing-events.md).

* **Schema-first subscriptions.** Consumers subscribe to a schema contract, not a bus. Nexus pins the latest version per environment, materializes EventBridge rules, and wires cross-account delivery with DLQs and zero-events alarms. Per-environment enable/disable maps to native rule state. See [Domain Management Guide](docs/domain-management.md#event-subscriptions).

* **Multi-environment reliability.** Every domain is provisioned across the platform-configured environment list (default: dev, staging, prod). Provisioning is all-or-nothing; deprovisioning is idempotent. See [Architecture](docs/development/architecture.md).

## Working with Nexus

The documentation builds understanding linearly — each step assumes the ones before it:

| # | Step | Guide | Audience |
|---|------|-------|----------|
| 1 | Understand the concepts | [Architecture](docs/development/architecture.md) — start with *The Problem* and *System Overview* | Everyone |
| 2 | Install the platform | [Installation Guide](docs/installation.md) | Platform operators |
| 3 | Onboard domain accounts | [Trust Stack Deployment](docs/trust-stack-deployment.md) | Platform operators |
| 4 | Register and approve domains | [Domain Management Guide](docs/domain-management.md) | Domain owners |
| 5 | Set up users and access | [OAuth Authentication Guide](docs/development/oauth-authentication-guide.md) | Platform admins |
| 6 | Publish events, manage schemas | [Publishing Events](docs/publishing-events.md) | Producers |
| 7 | Subscribe to events | [Domain Management Guide — Event Subscriptions](docs/domain-management.md#event-subscriptions) | Consumers |

Deep reference for contributors and operators: [Architecture](docs/development/architecture.md), [Decisions](docs/development/decisions.md), [Testing Guide](docs/development/testing.md), [Service Quotas](docs/service-quotas.md), [AGENTS.md](AGENTS.md).

> Default AWS quotas cap a fresh platform account at **3 domains**. See [Service Quotas](docs/service-quotas.md) before onboarding more.

## Current Scope

- **No domain deletion API.** Domain teardown is handled by `make teardown-domain`, which drives the same deprovisioning workflow used by the provisioning saga's rollback path. A `DELETE /domains` flow is planned.
- **Schema discovery follows entitlement coverage.** The consumer catalog (`GET /schemas?consumerDomain=…`) shows exactly what a domain's entitlements cover — publishing and schema writes are gated by per-domain role bindings, and subscribing additionally requires a covering entitlement. Platform roles get the platform-wide view.
- **No schema promotion between environments.** Publishers append versions per environment stage; promotion workflows are a future capability.
- **Business payloads are not validated against schema content.** The platform validates the envelope it generates, not your data — a deliberate design choice.

## Technology

Rust Lambda functions on ARM64 Amazon Linux 2023, AWS SAM infrastructure, DynamoDB, Step Functions with JSONata, and EventBridge.
