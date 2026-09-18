## 1. Establish validator-backed regressions

- [x] 1.1 Add the optional-property and reference-under-negation counterexamples as Rust regressions; verify the current checker accepts the pairs while the project's Draft-4 validator accepts each witness under the predecessor and rejects it under the candidate, retaining red-phase evidence.
- [x] 1.2 Add a validation-semantics matrix for numeric boundaries around 2^53, signed/unsigned values, fractional bounds and multiples, exclusive bounds, `format`, and `id`/reference scope; verify raw and representative composed-contract results with the existing Rust dependency versions and record any differences from the exploratory Python checks.
- [x] 1.3 Define positive regression cases for identical schemas with and without references, reference-free annotation edits, closed-object optional additions, required-set reduction, supported type/enum widening, and small integer constraint loosenings; verify the cases exercise the expected payload-validation behavior rather than only asserting checker output.

## 2. Make acceptance explicit and bounded

- [x] 2.1 Introduce an explicit accepted/not-established checker result with typed path-bearing reasons for local restrictions and unsupported proofs; verify tests no longer confuse inability to prove inclusion with a demonstrated globally breaking payload.
- [x] 2.2 Add the shared proof budget of 64 schema levels, 10,000 schema-node visits, and 100,000 comparison steps, including equality and collection comparisons; verify boundary and exhaustion tests fail closed without panics or partial acceptance.
- [x] 2.3 Add schema-position-aware reference and semantic-keyword guards before local acceptance; verify changed referenced documents, reference-target changes beneath negation, unsupported schema positions, and uncertain `id`/`format` changes reject, while identical referenced documents and literal `$ref` keys in instance data are handled correctly.

## 3. Repair property and numeric rules

- [x] 3.1 Compare candidate-only property constraints against the predecessor's effective additional-property policy; verify omitted/true/false/schema-valued cases, nested objects, and the optional-property counterexample, with a passing closed-object addition case.
- [x] 3.2 Reject candidate-only properties involving pattern-property reasoning that the bounded checker does not support; verify overlap cases cannot bypass the gate and retain existing conservative property-removal behavior.
- [x] 3.3 Replace lossy bound and divisibility proofs with exact typed/checked comparisons in the supported numeric domain and unproven results elsewhere; verify the numeric matrix from 1.2 and preservation of supported safe cases without epsilon tolerances or a validator upgrade.
- [x] 3.4 Add a bounded deterministic schema/payload corpus spanning interactions among properties, requirements, types, enums, arrays, constraints, combinators, and references; verify every checker-accepted pair preserves every sampled predecessor-valid payload, alongside documented per-rule justifications rather than treating sampling as proof.

## 4. Integrate the handler and document the boundary

- [x] 4.1 Wire the explicit checker result into schema-admin with accurate capped HTTP 400 reasons and the existing rejection metric; verify safe append success, the concrete unsafe pairs, unsupported proof and budget rejection, authorization ordering, latest-only lookup, no role bypass, no registry update or publication event on rejection, and unchanged create/missing-target behavior using handler mocks.
- [x] 4.2 Update both OpenAPI descriptions and `docs/publishing-events.md` for conditional optional-property acceptance, supported proof limits, and accurate rejection guidance; verify API-surface parity tests and examples agree with the delta spec.
- [x] 4.3 Update `docs/development/publish-validation.md` and the schema-gate description in `docs/development/architecture.md` to distinguish sound pairwise checking from atomic append safety; verify neither claims unconditional concurrent-append protection or a known deployed baseline.
- [x] 4.4 Update `docs/development/decisions.md` with the correctness-first proof domain and explicit separate follow-ups for concurrent-append serialization and issue #30's broader acceptance; verify those follow-ups are recorded without adding an append lock, solver, external issue write, or weakening the accepted-pair invariant.

## 5. Validate and reconcile specifications

- [x] 5.1 Run `rtk cargo test -p schema-admin -p konductor-common` and `rtk make check`; verify all relevant tests and checks pass, recording any baseline failures separately without claiming a failed command passed.
- [x] 5.2 Review acceptance paths and numeric/reference interactions with the Rust reviewer; verify actionable findings are addressed and no unrequested dependency, infrastructure, deployment, or unrelated-work changes are included.
- [x] 5.3 Sync the approved schema-management delta into the main specification during implementation and run `rtk openspec validate harden-schema-compatibility-checker --strict`; verify all retained and added scenarios match the implementation, all completed task claims have evidence, and concurrency remains a separately tracked limitation.
