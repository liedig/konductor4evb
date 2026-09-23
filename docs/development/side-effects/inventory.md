# Audit inventory and follow-ups

[Documentation home](../../README.md)

Dated records from the [side-effects audit](README.md). These are rationale and failure analysis, not proof that the present code still has this shape.

## `grant-benchmark`

- Non-production latency harness: unconditional fixture put/delete on the bindings table, no events. STAY—benchmark scaffolding, not a production write path; noted so the inventory is complete.

## Out of taxonomy (documented for inventory completeness)

- **`domain-count-reporter`** (added by `cap-domain-count` after this audit was drafted): no writes; hourly GetItem + structured log line feeding a metric filter. Schedule-driven; nothing to migrate.
- **Read-only handlers** (`describe-domain`, `list-domains`, `pending-registrations`, `list/describe-subscription`, `list/describe-schema`, `oauth-authorizer`): no writes, no emissions.
- **Step Functions state machines**: workflow-owned SDK task states write status directly—`approval.asl.yaml` (3 `updateItem` states for approval-path status transitions), `activate-subscription.asl.yaml` (2 stream-status updates), `remove-subscription.asl.yaml` (marks the record `REMOVED` + TTL). The execution context is their durable envelope (workflow-owned events are a standing decision from `domain-management-redesign`); they are orchestrators, not handler dual-writes, and were outside the mechanical handler inventory. Completeness note from the second pass: the `konductor-common` helper `streams_db::mark_stream_removed` has no callers—the removal workflow performs that update from ASL. Dead code; remove it in whichever subscription change lands next.
- **Crate census (second pass):** all 28 function crates plus `konductor-common` are accounted for above; `api/` holds OpenAPI specs only, and the web console is TypeScript calling the APIs (no direct writes).

## Rubric placement (task 3.3)

The audit caught authoring-time gaps in 5 of 14 classified call sites (two uncovered wedges, one stranded state, one defeated idempotency key, one duplicate-emission window)—all introduced before the rubric existed. The rubric therefore lands as an **authoring-time checklist in AGENTS.md** ("Side Effects and Durability"), the repo's single source of truth for conventions, with a single pointer line in the PR template checklist as the review gate.
