# Proposal: add-event-subscriptions

## Why

Domains can be registered and provisioned, but no events can flow: there is no way for a consumer to connect to a producer's event stream without hand-crafting EventBridge rules and cross-account policies — exactly the toil Nexus exists to eliminate. This change adds the subscription capability: schema-first subscribe/unsubscribe with per-environment enable/disable, making the schema registry the catalog of consumable contracts and completing the consumer half of the platform's publish/subscribe promise.

## What Changes

- **Subscribe API (schema-first).** Consumers subscribe a registered domain to another domain's event schema — `Subscribe(consumerDomain, producerDomain, schemaName)`. Subscribing always binds to the **latest** schema version, pinned per environment at subscribe time. Subscribers cannot choose historical versions: the schema is the contract, and there is only one current representation of an event.
- **Event stream materialization.** Each subscription materializes as one EventBridge rule per environment on the producer's platform-side bus: name `{consumerDomain}-{detailType}` (version-agnostic, unique per bus — uniqueness doubles as duplicate-subscription enforcement), pattern filtering on `source` + `detail-type` + numeric `detail.metadata.schemaVersion`, target the consumer's app-side domain bus, DLQ via the existing per-domain subscription DLQ (`DeadLetterConfig`).
- **Event streams registry (DynamoDB).** All event streams are registered in a new `nexus-event-streams` table — the source of intent, with EventBridge rules as materialization. Topology questions ("who subscribes to this schema, on which version?", "what feeds my domain?") are answered from the table, never by enumerating AWS resources. Removed streams are retained 90 days (TTL) for audit.
- **Per-environment enable/disable.** Subscriptions carry per-env desired state mapped to native rule `State` (e.g., dev/test active, prod paused for cost during development). Initial per-env state is settable at subscribe time; the same primitive serves as an operational kill switch.
- **Unsubscribe.** Removes platform-side rules across all environments and marks the stream record `REMOVED`. Orphaned integration rules on the consumer's app-side bus are the application team's responsibility (platform/app segregation).
- **Schema discovery (read-only).** Consumers can list and describe schemas in a producer domain's registry to find contracts to subscribe to. Schema *registration* remains out-of-band for v1 (domain owners write to their registry via AWS APIs, as already permitted by registry access policies).
- **Canonical event envelope.** Formalizes the platform event contract the rules filter on: `source` = the domain's env-independent namespace identity (`{org}-{domain}`), `detail-type` = event name, schema name = `{source}@{detail-type}` (EventBridge discovery convention, round-trips to `aws schemas describe-schema`), numeric schema version at `detail.metadata.schemaVersion`. The future publish capability must stamp and validate this envelope.
- **BREAKING (spec-level): subscription delivery mechanism.** Cross-account delivery uses a resource policy on the consumer's app-side bus granting the platform account `events:PutEvents` — attached once at provisioning, not per subscription. This **replaces** the previously specced per-subscription bus-to-bus assumed roles for subscription routing (never implemented; no runtime impact). `provision-env` gains the policy step. No backfill is needed: the product is unreleased, no environment carries pre-change domains, and any dev stack is rebuilt from scratch.
- **Async command processing for subscription writes.** Subscribe/unsubscribe/state changes follow the established write path (API Gateway → SQS → command handler → workflow) with `202 Accepted`, outcome events on the nexus bus, and status lifecycle `PENDING → ACTIVE | FAILED | REMOVED`.

## Capabilities

### New Capabilities

- `event-subscriptions`: Subscribe/unsubscribe lifecycle, per-env enable/disable, the `nexus-event-streams` table as source of intent, rule materialization and naming on producer platform buses, subscription identity and idempotency, ownership validation (caller must own the consumer domain), self-subscription support.
- `schema-management`: Read-only schema discovery for v1 — list/describe schemas in a domain's registry so consumers can find contracts. Establishes the bounded context that later changes extend with registration, evolution, and access control.
- `event-envelope`: The canonical event structure all Nexus events share — source/detail-type/schema-name derivation from platform-owned identity, numeric `detail.metadata.schemaVersion`, `metadata`/`data` split within `detail`. Shared contract between subscription filtering (this change) and publish stamping/validation (future change).

### Modified Capabilities

- `cross-account-trust-model`: **BREAKING** — the "Bus-to-bus assumed roles for subscription routing" requirement is replaced: subscription delivery is authorized by a provision-time resource policy on the consumer's app-side bus (platform account `events:PutEvents`); no per-subscription IAM roles are created or cleaned up.
- `resource-topology`: Subscription routing and event flow path requirements updated to the resource-policy delivery mechanism; subscription rule naming/filtering added to the platform bus topology.
- `domain-provisioning-workflow`: The per-env provisioning sequence gains a step — attach the platform-delivery resource policy to the app-side domain bus (via the existing trust-role `events:PutPermission`, no trust-stack change).
- `async-command-processing`: Command topology extended to route subscription write operations through a command queue with `HttpMethod` dispatch and `{Operation}Succeeded|Failed` outcome events.

## Impact

- **API**: `api/openapi.yaml` — new endpoints: `POST /subscriptions`, `GET /subscriptions` (filter by consumer/producer/schema/version), `GET /subscriptions/{id}`, `PATCH /subscriptions/{id}` (per-env enable/disable), `DELETE /subscriptions/{id}`, `GET /schemas` (discovery).
- **Infrastructure** (`nexus-core-stack.yaml`): new `nexus-event-streams` DynamoDB table (+ consumer-side GSI, TTL), subscription command queue + DLQ, new Lambda functions, subscription workflow (Step Functions) for multi-env rule creation with compensation.
- **Code**: new function crates (subscription command handler, list/describe subscriptions, schema discovery); `lib/nexus-common` — stream models, rule-name/pattern derivation alongside the existing `NamespaceResolver`.
- **Workflows**: `provision-env.asl.yaml` gains the app-bus resource-policy step (backfill for already-provisioned domains was descoped: no installed base exists pre-release).
- **Trust stack**: no change — existing `events:PutPermission` grant suffices; domain accounts do not re-onboard.
- **Docs**: `docs/development/architecture.md` (must be updated per project convention), README API table, domain-management guide.
- **Constraint pushed to subscribe-time validation (v1)**: event names must satisfy rule-name constraints (charset `[A-Za-z0-9._-]`, combined rule name ≤ 64 chars); moves to registration-time when schema registration comes in-platform.

### Out of Scope

- Schema registration, evolution, and versioning governance (separate thread; version pinning here is its enforcement substrate)
- Per-env schema version skew policy and stale-enable semantics (flagged follow-ups for the evolution thread)
- Schema registry RBAC / authorization layer (parked until the subscription model is proven)
- Custom event patterns per subscription (consumers filter via their own local rules), non-domain subscribers (arbitrary bus targets), SNS/SQS/Kinesis targets
- Publish API (next thread; consumes the `event-envelope` contract defined here)
