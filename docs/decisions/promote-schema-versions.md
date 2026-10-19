# Replay-based schema promotion with aligned version numbers

[Decision index](README.md)

- Recorded date: 2026-10-19.
- Change ID: `promote-schema-versions`.
- Status: recorded design, shipping the promotion workflow parked in
  [deferred](deferred.md) (`gate-schema-promotion`) and extending
  [follow-schema-promotion](follow-schema-promotion.md).

## Decision and rationale

Schemas are authored only in the first configured environment (the authoring
environment); every other environment receives versions exclusively through
explicit promotion. Promotion is a replay: a Step Functions workflow re-writes
the missing versions into the target registry under the *same* version
numbers, so a contract reference `vN` means the same schema everywhere. Replay
begins at the target's latest version and requires the shared prefix to be
byte-identical (canonical JSON); a mismatch stops the promotion as `DIVERGED`
without touching the target.

The request commits one conditional head item to the promotions table; the
stream consumer starts the workflow with the promotion id as the idempotent
execution name, and lifecycle events (`SchemaPromotion{Started,Completed,Failed}`)
ride the same stream. This keeps the API synchronous-write-only per the
[side-effects rubric](../development/side-effects/README.md).

Subscribers keep delivery continuity through a widen, drain, narrow re-pin:
subscription rules first widen to `[pinned..target+1]` (the last entry is a
lookahead so events published mid-replay still match), wait out a configured
drain, then narrow to the replayed version. Rule updates are performed by the
existing materialize-subscription-env Lambda in a `repin` mode, fanned out per
environment by a small `repin-schema-env` workflow started by the
subscription-extender on each `SchemaVersionPublished` in a promoted
environment. The re-pin execution name doubles as the widen owner, so a second
promotion supersedes an abandoned widen instead of stacking on it.

Authorization is a new `schema:promote` grant action checked against the
producer domain; the console offers promotion only on the producer schema
page and hides it from the platform auditor role.

## Evidence and limits

The [OpenSpec change](../../openspec/changes/promote-schema-versions/) keeps
the full proposal, design decisions, and spec deltas. Spike evidence (registry
serves LATEST only, canonical-JSON equality, `ClientTokenId` idempotency,
environment-derived API stages) is recorded in its design.md.

Limits: promotion advances one environment hop per request (source is the
target's configured predecessor); replay trusts the source registry's stored
content; the drain window is a fixed platform parameter
(`SchemaRepinDrainSeconds`, default 300s); and console stages assume the API
stage names equal the configured environment names.
