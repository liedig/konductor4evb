## Context

See proposal.md - Why. The classification rubric is the stream-driven side-effects principle and constraints established in `add-subscriber-entitlements` (design.md, "Stream-driven side effects (first application)"); this change applies that rubric to the existing codebase rather than restating it. Current state: one DynamoDB stream exists — the entitlements table stream feeding `entitlement-stream-consumer`, the rubric's first application; every other write path is handler-driven. Known multi-step handlers from a preliminary scan: `domain-registrations/register.rs` (two conditional puts — registration and collision preflight — then Step Functions start, then EventBridge emission, with hand-rolled idempotent continuation), `domain-registrations/update.rs` (write plus lifecycle event), `domain-state` (update plus lifecycle event), `finalize-approval` (transactional write on decision), `event-streams/subscriptions` subscribe/unsubscribe (write, then Step Functions start and EventBridge emission), `event-streams/subscriptions/outcome.rs` (outcome events after workflow results), and the control-plane admin handlers (`entitlement-admin`, `binding-admin`, `entitlement-revocation-worker`, `entitlement-coverage-sweep`) added after this audit was drafted. The publish path is already envelope-driven (inbound SQS command is the durable envelope).

## Goals / Non-Goals

**Goals:**

- A complete inventory of handler code paths that commit a DynamoDB write and perform additional side effects.
- A consistent per-handler classification against the rubric, including a failure-mode walk of each multi-step path.
- A recorded decision per handler in `docs/development/decisions.md`, durable enough that future readers understand why each handler looks the way it does.
- The entitlement-family handlers beyond the already-stream-driven revocation path (grant administration, binding administration, the revocation worker, the coverage sweep) are audit subjects like any other write path; only the revocation path is exempt as the rubric's first application.

**Non-Goals:**

- Migrating any handler (migrations spawn as their own changes after decisions).
- New infrastructure, tables, streams, or Lambdas.
- Reopening the `add-subscriber-entitlements` revocation design, which is the rubric's first application, not an audit subject.
- A mandate that everything must migrate; STAY is a valid recorded outcome.

## Decisions

### The rubric is applied as six questions plus a translation judgment

Per handler call site, answer: (1) does the written item carry everything a stream consumer would need, or would the consumer require handler-local state? (2) does every side effect correspond to a committed state transition, or are some emissions about rejections that never commit? (3) can the side effect be made idempotent on a stream event id? (4) does the side effect tolerate seconds of stream latency? (5) can the consumer meet the 24-hour retention and shard-blocking operational requirements? (6) is durability already provided by an inbound command envelope, making a stream redundant? Then judge each emitted artifact: change record (mechanical reaction, stays internal) or business event (domain fact for the control-plane bus, business-level audit, future broader sharing), proposing names in the established `{Operation}Succeeded|Failed` style where translation is warranted.

### Decisions are recorded in a fixed three-value taxonomy

Each handler receives exactly one of: **MIGRATE** (stream-driven is strictly better; a follow-up change is named), **STAY** (handler-driven remains correct; the reason is recorded, e.g. synchronous read-your-write latency, no committed transition to hang on), or **ENVELOPE-DRIVEN** (durability already comes from the inbound command queue; nothing to fix). A fixed taxonomy prevents the audit from producing prose that cannot be acted on.

### The decision record has a fixed format

Each entry in `docs/development/decisions.md` carries: call site, committed write(s), side effect(s), the six rubric answers in brief, the failure-mode walk result (what partial state a crash between steps leaves, and how current recovery covers it), the decision, rationale, and the follow-up change name when MIGRATE. Fixed format keeps the audit reviewable and diff-able.

### The audit is mechanical-first, then manual

Inventory starts from a mechanical search (DynamoDB write operations crossed with EventBridge, SQS, SNS, and Step Functions calls in the same handler crate), then each candidate is traced manually to confirm the write/side-effect graph and the existing recovery mechanism. Mechanical-first keeps the inventory complete; manual tracing keeps the classification honest.

## Risks / Trade-offs

- **[The audit rots as new handlers are written]** → Task 3.3 decides whether the rubric becomes an AGENTS.md or PR-review checklist so new write paths are classified at authoring time.
- **[Latency-sensitive or read-your-write paths get misclassified as MIGRATE]** → Rubric question 4 exists precisely to force STAY for those; the failure-mode walk must show a real, uncovered partial state before MIGRATE is recorded.
- **[Scope creep into migration work]** → Non-goal, enforced by the taxonomy: MIGRATE outcomes name a follow-up change instead of editing code.

## Migration Plan

No production deployment. Rollout is: decisions.md updated with the per-handler records, follow-up changes scaffolded for approved migrations, and (if task 3.3 decides so) the authoring-time checklist added to contributor docs. Rollback is reverting the documentation commit.

## Open Questions

None that block tasks. Whether the rubric lands in AGENTS.md versus PR templates is decided in task 3.3, after the audit shows how many handlers the checklist would have caught.
