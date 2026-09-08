# Proposal: follow-schema-promotion

## Why

Subscription activation is all-or-nothing across every producer environment, but the catalog lists a schema when it exists in *any* environment — so a dev-only schema is advertised, clickable, and un-subscribable by design. This happened in the live deployment: shipping's subscription to `acme-billing@OrderNotification` (present only in dev) failed wholesale on the staging registry 404, and the recorded reason (`errorDetail`) is dropped by the read APIs, leaving "failed for unknown reasons".

The product intent is that a subscription binds to the **schema**, not to a fixed environment set: consumers start developing against a schema the moment it exists anywhere, and when the producer promotes it to a further environment, existing subscriptions should extend into that environment automatically, with no consumer action.

## What Changes

- **BREAKING (spec contract)**: all-or-nothing multi-environment activation is removed. Activation materializes each environment *independently*, scoped to the environments where the schema actually exists. A schema absent from an environment no longer fails the subscription; that environment is recorded as awaiting promotion.
- Subscriptions follow promotion: when the producer creates or appends the schema in a new environment, existing subscriptions materialize there automatically (rule, target, alarm), enabled by default and honoring pause flags declared before the schema arrived.
- `schema-admin` emits a `SchemaVersionPublished` lifecycle event on every successful create/append, carrying domain, schema, environment, and version. This is the promotion signal.
- Per-environment enable/disable becomes desired-state management: pausing or resuming an environment where the schema has not landed yet is recorded and honored at promotion time instead of rejected.
- Subscribe fails fast with a clear reason when the schema exists in *no* producer environment (typo guard).
- Subscription read operations expose `errorDetail`, and the web UI surfaces activation/materialization failures and awaiting-promotion environments. Subscribing from a skewed catalog entry shows per-environment availability up front.

## Capabilities

### New Capabilities

(none — promotion-following is subscription behavior and extends the existing bounded context)

### Modified Capabilities

- `event-subscriptions`: activation scoping (presence-based, per-environment independent, no saga), per-environment materialization status including awaiting-promotion, promotion-following extension with final entitlement re-check, desired-state enable/disable semantics, lifecycle events, and the read surface (`errorDetail`).
- `schema-management`: schema-admin SHALL emit the `SchemaVersionPublished` promotion signal on successful create/append.
- `web-ui`: the catalog discovery / subscription journey surfaces per-environment availability, awaiting-promotion state, and failure detail.

## Impact

- **Code**: `subscriptions` handlers (`subscribe.rs`, `update_state.rs`), `activate-subscription.asl.yaml` (per-env independence; record update becomes a per-env merge instead of whole-map replace), a new subscription-extender Lambda plus control-plane bus rule, `schema-admin` handler (event emission), `list-subscriptions` / describe read surface, web catalog detail and subscriptions pages.
- **APIs**: `POST /subscriptions` per-environment map gains desired-state semantics for not-yet-materialized environments; `GET /subscriptions` responses gain `errorDetail` and a new per-environment status value; new control-plane bus event `SchemaVersionPublished`; new lifecycle event for environment extension.
- **Data**: event-streams records gain a new per-environment status variant. No migration: existing `ACTIVE` subscriptions are fully materialized already; the live `FAILED` record is re-subscribable today.
- **Infrastructure**: one EventBridge rule on the control-plane bus and one Lambda (IAM: schemas describe, EventBridge rule/target/alarm writes, DynamoDB stream update).
- **Related work**: complements the parked `gate-schema-promotion` proposal (that gates append-time compatibility; this change consumes promotion as a signal — the emitted event will serve both).
