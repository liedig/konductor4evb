# Design: Gate Schema Promotion

## Context

See proposal.md — Why. The relevant current state:

- `append_schema_version` in `event-streams/functions/schema-admin` runs `check_draft4_document` on the appended content, authorizes via `prepare`, then calls `UpdateSchema`. It never reads the registry, so it cannot compare against the current latest version.
- The SchemaAdmin function role holds `schemas:CreateSchema`/`schemas:UpdateSchema` only — the gate's latest-version read needs `schemas:DescribeSchema` added to that policy in `konductor-core-stack.yaml`.
- Admission (`publish-events-admission`) validates against a TTL-cached latest version; its soundness for the stale-acceptance case assumes additive-only evolution (`docs/development/publish-validation.md`). This change is what makes that assumption true.

## Goals / Non-Goals

**Goals:**

- Every registered version accepts every payload its immediate predecessor accepted, enforced at append time.
- Rejections are deterministic and actionable: `400 Bad Request` naming the offending instance paths.
- The compatibility checker is a pure function over two draft-4 documents — unit-tested with no AWS mocks.

**Non-Goals:**

- Semantic implication checking (accept v(N+1) iff its valid-instance set is a true superset of vN's). The syntactic rules are a conservative approximation; the semantic engine is a future refinement tracked as [issue #30](https://github.com/sliedig/konductor4evb/issues/30).
- Gating `POST /schemas` (version 1 has no predecessor) or comparing against historical versions (admission validates against latest, so latest is the only predecessor that matters).
- An override mechanism. The gate is absolute by product decision: breaking evolution is expressed as a new schema under a new event name, which is also the subscription-facing contract name, so consumers opt into a break explicitly.

## Decisions

### Syntactic rule set, not semantic implication

The checker walks the two documents and applies per-keyword rules under one principle: the candidate's accepted instance set must *visibly* contain the predecessor's; anything it cannot prove is rejected.

- `required`: candidate set ⊆ predecessor set (dropping is loosening; adding is tightening).
- `properties`: every predecessor-declared property must still be declared; matched properties recurse. New candidate-only properties pass. (Conservative: removing a property under `additionalProperties: true` does not actually tighten, but the v1 gate rejects all removals rather than reason about the interaction.)
- `type`: candidate type set ⊇ predecessor set.
- `enum`: candidate set ⊇ predecessor set; removing `enum` entirely passes.
- `minimum`/`maximum`/`exclusiveMinimum` (draft-4 boolean semantics), `minLength`/`maxLength`, `minItems`/`maxItems`, `multipleOf`: the candidate's admitted range must contain the predecessor's.
- `pattern`: identical or removed only — regex containment is undecidable in general, so any change rejects.
- `additionalProperties`: flipping `true`→`false` rejects; `false`→`true` passes.
- Combinators (`allOf`, `anyOf`, `oneOf`, `not`) and `$ref`: subtrees must be deep-equal between versions. Any change is unprovable under syntactic rules and rejects.

Alternatives considered: a semantic subset engine (true guarantee, but no mature Rust implementation and poor error messages; deferred); comparing against every historical version (stronger than the invariant admission needs, and would freeze early mistakes into the contract forever); a JSON Schema "diff" library (none exists for draft-4 compatibility semantics).

### Checker lives in `konductor-common` as a pure module

`schema_compatibility::check_compatible(predecessor: &Value, candidate: &Value) -> Vec<Incompatibility>` (empty = compatible), each `Incompatibility` carrying the instance path and the rule violated. Alternatives: inline in the schema-admin handler (rejected — handler code is mock-heavy and the rules deserve a dense unit-test matrix); inside `schema_validation` (kept separate — validation answers "does this payload conform", compatibility answers "does this document subsume that one"; different callers, different evolution).

### Gate placement in `append_schema_version`

After `check_draft4_document` and `prepare` (so unauthorized callers learn nothing about schema contents), fetch the current latest with a version-less `DescribeSchema` on the target registry, run the checker, and only then `UpdateSchema`. The fetch doubles as the existence check: its `NotFoundException` maps to the established 404, with the `UpdateSchema` NotFound mapping retained as defense. No caching — appends are rare, and a stale predecessor read would weaken the gate.

### Error contract

`400 Bad Request` via the existing `api::validation_error` shape, message listing each offending path and rule (capped like `MAX_REPORTED_VIOLATIONS` so a wholesale rewrite cannot produce an unbounded body). This matches the sibling content rejections on the same route and the spec's "identifying the offending paths".

## Risks / Trade-offs

- [False rejections of provable-by-hand loosenings the rules cannot see (pattern widening, combinator edits, property removal under `additionalProperties: true`)] → Documented in the producer guide with the new-event-name escape; a `SchemaPromotionRejected` metric makes over-gating observable, and rules can be refined without weakening the invariant.
- [Race between the gate's `DescribeSchema` and a concurrent append to the same schema: two candidates both check against vN, and the second-registered version may tighten relative to the first] → Accepted and documented. The Schemas API offers no conditional write; appends to one schema are publisher-team-coordinated and rare, and per-environment registries bound the blast radius. If it ever bites, a per-schema append lock in the control plane is the follow-up, not a reason to skip the gate.
- [New `DescribeSchema` permission on the schema-admin role widens its read surface] → Read-only, scoped to `${Organization}-*` registries/schemas like the other functions; no publisher-visible data leaves the account.
