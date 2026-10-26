# Konductor for Amazon EventBridge

<img src="docs/konductor-logo-light-bg.svg" alt="Konductor for Amazon EventBridge" width="500">

[![CI (main)](https://github.com/liedig/konductor4evb/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/liedig/konductor4evb/actions/workflows/ci.yml?query=branch%3Amain)
[![CI (develop)](https://github.com/liedig/konductor4evb/actions/workflows/ci.yml/badge.svg?branch=develop)](https://github.com/liedig/konductor4evb/actions/workflows/ci.yml?query=branch%3Adevelop)
[![License: Apache-2.0](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

A serverless control plane for cross-account event-driven architecture at scale. Platform teams get governed domains — registration, approval, provisioning, ownership, and observability. Application teams get three simple verbs — **publish, subscribe, unsubscribe** — for event stream automation and never touch event buses other than their own, IAM policies, or cross-account delivery infrastructure.

[Documentation home](docs/README.md) | [Installation](docs/operations/installation.md) | [Architecture](docs/architecture/README.md) | [API contracts](api/) (OAuth: `openapi-oauth.yaml`, IAM: `openapi-iam.yaml`)

## Why Konductor

Consuming an event from another AWS account means coordinating event buses, rules, resource policies, IAM roles, and DLQs across two accounts — 12+ resources and days of infrastructure wrangling per integration, reinvented by every team with subtle drift. Konductor abstracts that away so teams focus on their events, not their plumbing.

**Minutes, not days.** Subscribe to an event and start processing it in minutes. Cross-account delivery is invisible.

**Governed domains.** Every domain gets event buses, schema registries, DLQs, and CloudWatch monitoring, provisioned automatically across all environments with all-or-nothing saga rollback. Human approval gates registration.

**Simple verbs, real durability.** Publishing is one `POST /publish` call. The event is durably queued before anything downstream can lose it, and retries preserve its idempotency key and original publish timestamp.

**Schema-first contracts.** Consumers subscribe to a schema contract, not a bus. Publishers author schemas through the API in the first environment — append-only, with every new version checked for compatibility — and promote them to later environments. Konductor validates every publish against the latest version, pins subscriptions to it, and materializes the delivery rules.

**Built-in ownership and observability.** Every domain has a primary owner and required secondary owners. Approval workflows, lifecycle events, CloudWatch dashboards, and per-subscription alarms come standard.

## How It Works

```text
┌── CONTROL PLANE ─────────────────────────────────────────────────────────────────────┐
│  OAuth + IAM APIs and web console · grant engine (bindings + entitlements) ·         │
│  audit on every mutation · platform metadata revisions · revocation worker +         │
│  hourly sweep · operator alarms                                                      │
└─────────────────────────────┬─────────────────────────────────────────┬──────────────┘
                              │ authorizes every command                │ disables streams
                              ▼                                         ▼ that lose coverage
┌── DOMAIN MANAGEMENT ────────┴──────────┐   ┌── EVENT STREAMS ─────────┴──────────────┐
│  register → approve → provision every  │   │  schemas: authored in the first env,    │
│  environment atomically, each in its   │   │  appends compatibility-gated, then      │
│  pinned account: buses, registries,    │──▶│  promoted env → env with the same       │
│  DLQs, and monitoring in the platform  │   │  version numbers                        │
│  and domain accounts                   │   │                                         │
│                                        │   │  publish: one call, authorized and      │
│  approval in the console or API; email │   │  validated against the latest schema    │
│  only notifies the approvers           │   │  before the 202, then durably queued    │
│                                        │   │                                         │
│  lifecycle events and owner            │   │  subscribe: catalog shows only what     │
│  notifications on every transition     │   │  entitlements cover; per-env rules      │
│                                        │   │  deliver cross-account; re-pinned on    │
│  teardown reuses the deprovision       │   │  promotion; pause/resume per env        │
│  workflow (make teardown-domain)       │   │                                         │
└────────────────────────────────────────┘   └──────────────────────────┬──────────────┘
                                                                        │ upgrade notices
                                                                        ▼
                                             ┌── SYSTEM EVENTS ─────────┴──────────────┐
                                             │  subscription upgrades publish to the   │
                                             │  system-event bus and route to each     │
                                             │  domain's own SNS topic (SQS, Lambda,   │
                                             │  or email); opt-in per installation     │
                                             └─────────────────────────────────────────┘
```

Konductor runs in a dedicated platform account. Domain resources are provisioned into domain accounts through the cross-account trust role, and a registration can pin each environment to its own account. All control-plane state lives in the platform account; domain accounts contain only the resources their domains need.

Both API surfaces serve **one stage per configured environment** (default `dev`, `staging`, `prod`; see [stage rendering](docs/reference/configuration.md#stage-rendering)): the stage in the URL selects the target environment, so callers never name an environment, registry, or bus in a request. Schema writes are accepted only on the first environment's stage; later environments receive versions through promotion.

## Core Capabilities

- **Domain lifecycle.** Register a domain through the console or API. A platform administrator approves it in the console or through the API (an email notifies the approver), and Konductor provisions all resources across every configured environment with all-or-nothing saga rollback. See the [domain guide](docs/guides/domains.md).

- **Event publishing.** `POST /publish` with a domain, event name, and business data — Konductor authorizes the publisher, validates the payload against the latest schema version (`422` with the violations when it does not conform), stamps the canonical envelope, and delivers to the domain's event bus. Store-and-forward through SQS: once accepted (`202`), the event survives downstream failures and is retried with its idempotency key intact. See the [publishing guide](docs/guides/publishing-events.md) and [publish validation](docs/architecture/publish-validation.md).

- **Schema management.** Publishers create schemas and append versions with `POST /schemas` and `POST /schemas/versions` on the first environment's stage — append-only, JSON Schema draft-4, and compatibility-checked, in platform-managed registries. `POST /schemas/promotions` replays those versions into the next environment under the same version numbers. See the [publishing guide](docs/guides/publishing-events.md) and [schema promotion](docs/operations/schema-promotion.md).

- **Schema-first subscriptions.** Consumers subscribe to a schema contract, not a bus. Konductor pins the latest version per environment, materializes EventBridge rules, and wires cross-account delivery with DLQs and zero-events alarms. When a newer version is appended or promoted, Konductor re-pins the rules (widen, drain, narrow) so delivery stays continuous. Per-environment enable/disable maps to native rule state. See the [subscription guide](docs/guides/subscriptions.md).

- **System events.** Committed subscription upgrades are published as versioned public contracts (source `konductor.system`) and routed to each domain's own SNS topic, which teams subscribe to from SQS, Lambda, or email. Emission is opt-in per installation (`SystemEventEmissionEnabled`, off by default). See the [system events guide](docs/guides/system-events.md).

- **Multi-environment reliability.** Every domain is provisioned across the platform-configured environment list (default: dev, staging, prod). Provisioning is all-or-nothing; deprovisioning is idempotent. See [Domain lifecycle](docs/architecture/domain-lifecycle.md).

## Documentation

<a id="working-with-konductor"></a>

Start at the [documentation home](docs/README.md), which routes by reader task instead of reading order:

| I want to… | Start with |
|---|---|
| Understand the system | [Architecture overview](docs/architecture/README.md) |
| Install or operate the platform | [Operations](docs/operations/README.md): installation, trust, guardrails, access, quotas |
| Register and manage a domain | [Domain guide](docs/guides/domains.md) |
| Publish events or author schemas | [Publishing guide](docs/guides/publishing-events.md) |
| Consume another domain's events | [Subscription guide](docs/guides/subscriptions.md) |
| Look up an API, status, event, or table | [Reference](docs/reference/README.md) |
| Change or review the implementation | [Contributor guide](docs/development/README.md) and [AGENTS.md](AGENTS.md) |
| Understand why a choice was made | [Decision index](docs/decisions/README.md) and the [changelog](CHANGELOG.md) |

## Web Console

Konductor ships a role-gated web console (`web/`, Nuxt + Tailwind, statically hosted on S3 + CloudFront from the core stack). Platform admins land on the pending-approvals inbox; domain teams land on their domains. Sign-in is a custom Konductor-branded page using Cognito SRP — no hosted UI.

- **Develop**: `make web-dev` (`http://localhost:3000`). It writes `web/public/config.json` from the core stack outputs and proxies `/dev/*` API calls to the deployed OAuth API, so the dev console behaves like the deployed one.
- **Deploy**: `make deploy-web` — builds, syncs to the web bucket, writes `config.json` from current stack outputs (no rebuild needed on stack changes), and invalidates CloudFront. The console URL is the `KonductorWebUrl` output.
- **Test logins**: `make seed-users` creates the dev personas (`alice` = platform-admin, `bob` = team-payments owner).
- Visual language: [Design System](docs/development/design-system.md); the `/design-system` route renders every primitive against the live build.

> Default AWS quotas cap a fresh platform account at **3 domains**. See [Service Quotas](docs/operations/service-quotas.md) before onboarding more.

## Current Scope

- **No domain deletion API.** Domain teardown is handled by `make teardown-domain`, which drives the same deprovisioning workflow used by the provisioning saga's rollback path. A domain whose registry still holds schemas, or whose platform or application bus still carries rules, needs those removed by hand first (see [domain recovery](docs/operations/domain-recovery.md#schemas-and-event-bus-rules-block-deprovisioning)). A `DELETE /domains` flow is planned.
- **Schema discovery follows entitlement coverage.** The consumer catalog (`GET /schemas?consumerDomain=…`) shows exactly what a domain's entitlements cover — publishing and schema writes are gated by per-domain role bindings, and subscribing additionally requires a covering entitlement. Platform roles get the platform-wide view.

## Technology

Rust Lambda functions on ARM64 Amazon Linux 2023, AWS SAM infrastructure, DynamoDB, Step Functions with JSONata, and EventBridge.
