# Schema compatibility gate hardening

[Documentation home](../README.md)

- Recorded date: 2026-09-14
- Change ID: `harden-schema-compatibility-checker`
- Status: recorded decision; see the linked change for scope and validation.

## Decision and rationale

Correctness precedes broader acceptance. The syntactic gate shipped in `gate-schema-promotion` accepted schema pairs that reject previously valid payloads; two validator-backed witnesses (an optional property constraint on an open object, and a referenced-target loosening beneath `not`) pinned the defect before any repair.

The checker returns an explicit accepted or not-established result. Path-specific reasons distinguish observed local restrictions from unsupported proofs. A failed proof does not demonstrate incompatibility.

The checker applies the following rules:

- Evaluate candidate-only properties against the predecessor's effective additional-property policy. Optional means absence is allowed, not that values are unconstrained.
- Reject changes to documents containing schema-bearing `$ref`s before applying local rules. Loosening a referenced target can tighten a constraint under negation.
- Treat `id` and `format` as significant. The validator enforces `format`, and `id` changes reference resolution. The validation-semantics matrix records both behaviors.
- Compare numeric bounds and `multipleOf` exactly. Do not collapse values through `f64` at 2^53 or use rounded-quotient divisibility.
- Bound proof depth, node count, and work steps. Fail closed when any budget is exhausted.

A bounded schema and payload corpus detects counterexamples across rule interactions. Sampling never authorizes an append.

The guarantee is deliberately **pairwise**: the candidate is proven against the latest version the handler read, and that read is not atomically bound to the registry write. Serializing competing appends per registry/schema (or an equivalent conditional-commit protocol) is a separate follow-up, as is issue #30's broader semantic acceptance—neither may reuse a fast path this change removed. The gate still runs before any registry write, so the stream-driven durability rubric has no new paths to classify.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-09-16-harden-schema-compatibility-checker/proposal.md).

[Decision index](README.md)
