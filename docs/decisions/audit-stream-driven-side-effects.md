# Audited every DynamoDB write path against the stream-driven side-effects rubric; 3 MIGRATE, 1 idempotency fix, rest STAY/ENVELOPE-DRIVEN

[Documentation home](../README.md)

- Recorded date: 2026-08-25
- Change ID: `audit-stream-driven-side-effects`
- Area: Cross-cutting
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

Full per-handler records in the audit section below. Three migrations named (`stream-driven-domain-registration`, `stream-domain-lifecycle-events`, `stream-subscription-outcomes`) plus one small fix (`fix-unsubscribe-execution-idempotency`). The rubric becomes an authoring-time checklist in AGENTS.md.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-08-26-audit-stream-driven-side-effects/proposal.md). The recorded date above is not necessarily the archive or merge date.

## Current relevance

Full per-handler records now live in [the side-effects reference](../development/side-effects/README.md). Its dated classifications are not automatically current.

[Decision index](README.md)
