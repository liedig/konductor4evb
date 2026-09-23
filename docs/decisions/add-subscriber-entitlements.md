# Subscriber entitlement store, coverage-gated subscriptions, revocation pipeline, entitlement-expanded catalog

[Documentation home](../README.md)

- Recorded date: 2026-08-11
- Change ID: `add-subscriber-entitlements`
- Area: Authorization / Event Streams
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

**Entitlements complete the two-store model**: `konductor-authorization-entitlements` (PK consumer domain, SK `producerDomain#scopePattern`, `by-producer` GSI, by-entitlement GSI, TTL) is the subscriber half of the `add-grant-engine` split. Entitlements are environment-agnostic durable intent; `ExpiresAt` drives TTL removal at most ~4 days after expiry (DynamoDB deletes in its own window) and expiry is also evaluated logically at read time.

**Scope-pattern grammar**: one additive grammar serves coverage, subscription authorization, and catalog expansion—`*` covers a whole producer domain including future schemas, an exact detail type covers all versions of one schema including future versions, and a `*` suffix covers a prefix family including future members. Patterns deliberately share by naming convention: prefixes like `Order`/`OrderItem` overlap, so name producer schemas with hygiene and treat an unintended match as one entitlement delete away—no separate catalog-language.

**Entitlements are control-plane intent; EventBridge rules are compiled enforcement**: administration only writes table rows; the activation workflow compiles intent into a rule per environment, so intent travel is never blocked by enforcement failure and enforcement carries no per-subscription authorization state.

**Composite resource grammar**: `subscription:environment:{id}:{env}` resources make the engine evaluate create/enable under binding *and* coverage but disable/delete under binding alone—a lapsed entitlement can never wedge an existing subscription in an unmanageable state.

**OAuth-only administration with producer self-service**: create and revoke are authorized by the `platform-admin` global role *or* a `publisher` binding on the entitlement's producer domain (so producer teams share their own schemas without platform involvement); consumer-scoped lists additionally admit a `subscriber` binding. The IAM surface exposes no entitlement routes in this capability version—administration that changes other domains' access is an interactive, auditable act.

**Stream-driven side-effect transport**: the entitlements DynamoDB stream carries change records—small self-describing control messages routed by record type (audit events to `konductor-bus`, revocation messages to SQS)—never business events; the two planes never mix, so control throughput, ordering, and retention tune independently of event traffic.

**Revocation is reconciliation-bounded**: revoke is durable at the transaction commit and effective at the next decision immediately; the worker pipeline (stream → queue → disablement) converges existing subscriptions with idempotent writes, marker-claimed stream records (replay = no-op), and per-record partial batch failure; the hourly sweep scans every stream, re-evaluates coverage fail-closed, and disables uncovered ACTIVE subscriptions—drift windows are observable via alarms, never silent.

**403-over-404**: uncovered schema targets and unauthorized subscription commands answer `403 Forbidden` with a generic body—the denial is deterministic and never oracles the existence of producer schemas or other callers' subscriptions.

**`IamOnlyAuthorizer` retention**: platform-operated paths that never represent a domain owner keep the IAM guard rather than being force-migrated onto bindings they would tautologically satisfy.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-08-11-add-subscriber-entitlements/proposal.md). The recorded date above is not necessarily the archive or merge date.

[Decision index](README.md)
