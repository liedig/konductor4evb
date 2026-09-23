# Independent subscription materialization

[Decision index](README.md)

- Recorded date: 2026-09-08.
- Change ID: `follow-schema-promotion`.
- Status: recorded design, superseding the cross-environment subscription saga
  in [add-event-subscriptions](add-event-subscriptions.md).

## Decision and rationale

Each environment materializes independently through a shared Lambda. An absent
schema records `AWAITING_PROMOTION`; a failed materialization records its own
failure. A successful environment is not rolled back because another fails.

The shared implementation serves initial activation and promotion-following
extension. This avoids duplicating materialization and coverage logic in a
state machine and an extender. Per-environment merges avoid replacing sibling
state. Existing materialized environments are not re-pinned by later appends.

`SchemaVersionPublished` is handler-emitted after a registry write because the
registry has no DynamoDB stream. Its durability exception and failure walk are
recorded in the [schema write-path audit](../development/side-effects/publishing.md#schema-admin).

## Evidence and limits

The [OpenSpec design](../../openspec/changes/archive/2026-09-08-follow-schema-promotion/design.md)
preserves all seven decisions, alternatives, risks, and the migration plan.
Producer-side promotion automation and version migration are outside this choice.
The [consumer-guide conflict](../reference/documentation-discrepancies.md#d01)
is still awaiting separate review.
