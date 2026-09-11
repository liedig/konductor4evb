# Proposal: Gate Schema Promotion

## Why

The publish pipeline's cache-safety argument rests on an additive-only evolution norm: any payload valid under schema version N must remain valid under every later version. Only then can admission's TTL cache never admit an event the current contract forbids (the stale-*acceptance* case in `docs/development/publish-validation.md`). Today nothing enforces that norm — schema administration gates draft-4 syntax only, so a publisher can append a tightening version (a newly required field, a narrowed type), and a warm admission cache will keep admitting payloads that violate the now-current contract until its TTL expires. The norm that makes the cache sound is a convention; this change makes it a mechanical gate.

## What Changes

- **Append-time compatibility gate**: `POST /schemas/versions` compares the appended document against the current latest version in the stage-selected registry and rejects tightenings with `400 Bad Request` identifying the offending instance paths. Compatible appends proceed unchanged.
- **Syntactic additive-only rules (v1)**: the gate rejects removed properties, newly required properties, narrowed types, and tightened constraints (raised `minimum`, lowered `maximum`, shrunken `enum`, `additionalProperties` flipped to `false`, and their draft-4 kin); it allows new optional properties, dropped `required` entries, widened types, and loosened or removed constraints.
- **No escape hatch**: the gate is absolute — no override flag, no privileged bypass. Intentional breaking evolution means registering a new schema under a new event name, which keeps the per-schema promise "every published version accepts everything its predecessors accepted" unconditional.
- **Documentation**: `docs/development/publish-validation.md` and the producer guide are updated to state the norm is now enforced at append time rather than parked as a convention.
- Semantic subset checking (accept v(N+1) iff every payload valid under vN stays valid, beyond the syntactic rules) is a deliberate non-goal for v1; it is recorded in `design.md` as a future refinement and tracked as a GitHub issue.

## Capabilities

### New Capabilities

(none — this change tightens an existing capability)

### Modified Capabilities

- `schema-management`: the flat append-only schema version API gains a backward-compatibility gate — appends that tighten the contract relative to the current latest version are rejected with `400 Bad Request`; creation and discovery are unchanged.

## Impact

- **Code**: `event-streams/functions/schema-admin` — `append_schema_version` fetches the current latest version (`DescribeSchema`) and runs the compatibility check before `UpdateSchema`; `konductor-common` gains a schema-compatibility module (pure functions over draft-4 documents, unit-tested without AWS mocks).
- **APIs**: `POST /schemas/versions` on both the OAuth and IAM surfaces gains a `400` compatibility rejection (`api/openapi-oauth.yaml`, `api/openapi-iam.yaml` document it).
- **Docs**: `docs/development/publish-validation.md` (stale-acceptance section now cites enforcement, not a parked follow-up), `docs/publishing-events.md` (producer-facing evolution rules).
- **Semantics**: the additive-only evolution norm moves from convention to contract; the admission cache's soundness no longer depends on publisher discipline.
- **Out of scope**: `POST /schemas` (version 1 has no predecessor), subscription version pinning, consumer-side migration tooling, semantic implication checking.
