# Design: follow-schema-promotion

## Context

See proposal.md — Why. The moving pieces today: `subscriptions/src/subscribe.rs` snapshots all producer environments into the stream record and starts `activate-subscription.asl.yaml`; the ASL Map describes the schema in every env (even disabled ones — load-bearing, because `update_state` only flips pre-materialized rule state), re-checks entitlement coverage in inline JSONata, creates rule/target/alarm, and rolls everything back if any env fails. `schema-admin` is the sole writer of the platform-owned per-env registries and emits no events. `EnvStream` already models per-env `{pinnedVersion, ruleName, ruleArn, enabled, status}` with nullable `ruleArn`/`pinnedVersion`.

## Goals / Non-Goals

**Goals:**
- Activation scoped to environments where the schema exists; each environment materializes independently (user ruling: per-env independence).
- Promotion-following: a schema landing in a new environment extends existing subscriptions there, enabled by default, honoring pre-declared pauses (user ruling).
- Promotion signal emitted by `schema-admin` (user ruling: handler-emitted).
- Failure detail (`errorDetail`) exposed on subscription reads and surfaced in the UI (user ruling).
- One implementation of per-env materialization and of the entitlement coverage check.

**Non-Goals:**
- Version re-pinning / migration of already-materialized environments (the spec'd "future evolution operation").
- Producer-side promotion automation or promotion gating (parked `gate-schema-promotion` change; this change's emitted event will serve it).
- CloudTrail-based drift detection for out-of-band registry writes (no out-of-band writer exists: registries are platform-account resources written only by `schema-admin`).
- Cross-environment name mapping (environment names are a symmetric platform convention).

## Decisions

### 1. Presence is evaluated in the workflow, not the subscribe command

`subscribe.rs` keeps writing one pending entry per producer environment and starts the workflow. Inside the per-env materialization, a `ResourceNotFound` describe means *absent* → the env entry becomes `AWAITING_PROMOTION`; any other describe/materialization error → per-env `FAILED` with the cause. Alternative considered: handler-side presence discovery (rejected — adds AWS calls and failure modes to the command path and splits presence logic across two places; the workflow already describes per env).

Typo guard: if *no* environment materializes and none is merely awaiting because of transient error — i.e., the schema exists nowhere — the stream becomes `FAILED` with `errorDetail` naming the schema and producer. (`AWAITING` everywhere implies exists-nowhere, since a schema is always created into one registry at authoring time.)

### 2. Per-env independence; the saga is deleted

Each environment's materialization commits its own record entry and outcome. There is no cross-env compensation: a staging failure leaves a live dev rule, which is the desired behavior ("develop straight away"). Top-level status: `ACTIVE` when at least one env materialized, else `FAILED` with `errorDetail` summarizing the first failure. The existing rollback states in the ASL are removed.

### 3. Per-env materialization is extracted into one Lambda

A `materialize-subscription-env` Lambda owns the per-env unit of work: resolve latest version → final coverage re-check (skipped for disabled envs and self-subscriptions, as today) → `PutRule` (upsert, version pinned, state per desired flag) → `PutTargets` (consumer app bus, DLQ) → zero-events alarm → per-env record merge → per-env outcome. Both the activation workflow (Map invokes it per env) and the promotion extender invoke it.

Rationale over alternatives:
- *Status quo ASL inline steps + a duplicated extender implementation*: materialization logic would live in two places (JSONata + Rust).
- *Extender starts the existing ASL with a single-env input*: the ASL's terminal states transition whole-stream status, which does not fit "one env extended on an already-ACTIVE stream".

Two structural wins fall out: the ASL's whole-map `environments = :envs` replace (which would clobber sibling envs on extension) disappears — the Lambda merges its own env key; and the inline JSONata entitlement re-implementation (substring prefix matching) is replaced by the real grant engine `covers()` in konductor-common, ending a logic-duplication risk.

### 4. Promotion signal: `SchemaVersionPublished`, handler-emitted

`schema-admin` emits `{source: konductor.schema-management, detailType: SchemaVersionPublished, detail: {producerDomain, schemaName, environment, version, publishedAt}}` on the control-plane bus after every successful create/append. The extender's bus rule matches it; extension is idempotent (conditional per-env update: only entries still absent/awaiting), so emits on both create and append are fine — appends to already-materialized envs are no-ops by record state, and version bumps deliberately do not re-pin.

Side-effects rubric: schema-admin's committed write is the registry itself; there is no DDB write whose stream could carry this event, so handler-emission is the honest path. Record the exception in `docs/development/decisions.md`.

### 5. The extender

Control-plane bus rule on `SchemaVersionPublished` → extender Lambda: query the event-streams table for `ACTIVE` subscriptions on (producerDomain, schemaName), and for each whose record lacks the promoted env (or has it `AWAITING_PROMOTION`), invoke materialization for that env, then emit `SubscriptionEnvironmentExtended` (identity, env, pinned version, enabled). Streams in `PENDING` (activation owns them), `FAILED` (terminal; re-subscribe), and `REMOVED` are skipped. The final coverage re-check inside materialization closes the revoke-between-promotion-and-extension race, fail-closed as today.

### 6. `update_state` becomes desired-state reconciliation

All producer envs are always present in the record (materialized or awaiting), so `POST /subscriptions/state` semantics extend naturally: setting `enabled` records desired state; if the env is materialized, flip the rule state as today; if `AWAITING_PROMOTION`, there is no rule to flip — the flag is honored when promotion arrives (this is how "pause prod before the schema reaches prod" becomes expressible). Setting `enabled: true` on a per-env `FAILED` entry re-triggers materialization — this is the heal path for transient per-env failures, which re-subscribe cannot provide (it is an idempotent no-op on `ACTIVE`).

### 7. Read surface and UI

`list-subscriptions` summaries and subscription detail gain `errorDetail` (omitted when absent). The web subscriptions views render it for failed streams/envs; the catalog detail journey shows which envs will materialize now versus await promotion.

## Risks / Trade-offs

- [Promotion event lost between bus and extender] → env never extends → Rule target gets a DLQ; materialization is idempotent so replay is safe; a future census reconciler can diff registry presence against records (noted, not built).
- [Extension races unsubscribe: rule created after teardown] → orphan rule not in the record → Unsubscribe deletes by deterministic rule name (`{consumer}-{detailType}` on the per-env producer bus) rather than only from record state, closing the hole deterministically.
- [Fan-out cost on a popular schema's promotion] → bounded by subscription counts (low hundreds at scale target); extender processes sequentially with per-item conditionals.
- [Readers see `AWAITING_PROMOTION` before writers are deployed everywhere] → single deployment unit (SAM stack); pre-production, no mixed-version window to manage.
- [Per-env `FAILED` entries confuse consumers used to binary status] → `errorDetail` surfacing (decision 7) plus the `update_state` heal path (decision 6) make failures visible and recoverable.

## Migration Plan

1. Deploy the stack (new Lambda, bus rule, ASL rewrite, handler changes). Existing `ACTIVE` subscriptions are fully materialized already and are untouched.
2. Record the schema-admin emission exception in `docs/development/decisions.md`.
3. The live `FAILED` stream (shipping ↔ `acme-billing@OrderNotification`) is re-subscribed post-deploy: dev materializes, staging/prod await promotion.
4. Rollback: remove the extender rule and redeploy the prior stack. Awaiting entries become inert; no data migration is required in either direction (the new per-env status is only written by new code).
