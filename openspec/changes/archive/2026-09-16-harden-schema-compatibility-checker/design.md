## Context

See `proposal.md` for motivation. The current gate compares parsed Draft-4 documents after authorization and offline-composability checks and before the registry update. Its empty incompatibility list means acceptance. The implementation lives in `schema_compatibility/{mod,rules,structure}.rs`; the handler prefixes every rejection with a global incompatibility claim.

Exploration established two concrete counterexamples:

1. Predecessor `{"properties":{"sku":{}}}` and candidate `{"properties":{"sku":{},"note":{"type":"string"}}}`: payload `{"sku":"A1","note":42}` is accepted before and rejected after. The existing `new_optional_property_passes` test expects this pair to pass.
2. Predecessor `{"properties":{"a":{"type":"integer"},"b":{"not":{"$ref":"#/properties/a"}}}}` and candidate with `a` changed to `{}`: payload `{"a":1,"b":"text"}` is accepted before and rejected after. The checker accepts the local type removal while the unchanged `not`/reference text hides the changed meaning.

An installed Python Draft-4 validator confirmed these payload results in memory. No new Rust tests ran during exploration; implementation must reproduce them through the project's actual validation configuration. Numeric comparison currently converts bounds and `multipleOf` values to `f64`; the observed collision between distinct integers above 2^53 is a risk to test, not yet a demonstrated Rust-validator mismatch.

## Goals / Non-Goals

**Goals:**

- Make every acceptance rule justify inclusion of the predecessor's accepted instances within the candidate's, for the compared pair and supported validation semantics.
- Reject when the available rules cannot establish inclusion, while retaining useful paths and supported safe evolution.
- Validate acceptance rules against the same Rust Draft-4 and composed-contract paths used by the application.

**Non-Goals:**

- General implication solving, regex containment, SMT, finite-enum semantic fallback, or broader combinator normalization. These remain issue #30 follow-ups.
- Reference-graph implication, dereferencing arbitrary recursive documents, or schema rewriting.
- Concurrent-append serialization, historical registry repair, admission cache redesign, infrastructure mutation, deployment, or external issue writes.

## Decisions

### 1. Proof failure is not proof of incompatibility

Replace the implicit empty-error-list contract with an explicit accepted versus not-established result carrying typed, path-bearing reasons. Reasons distinguish local restrictions from unsupported proof cases. A local keyword restriction alone does not prove a globally breaking change: other constraints may already exclude every affected instance.

Preserve the existing HTTP 400 envelope and maximum of ten reported paths. Use a neutral prefix such as "backward compatibility could not be established" and accurate reasons such as "new property constraint is not proven to accept previously allowed values" or "reference-dependent change is unsupported". Do not claim a concrete counterexample unless one has actually been validated against both complete documents. This change does not introduce runtime counterexample search; validator-backed witnesses belong in regression tests.

Keep the `SchemaPromotionRejected` event field and add a bounded reason classification if needed, without logging schema documents or payloads. Unsupported cases are rejections, not internal errors or bypasses.

Alternative rejected: retain the old fast path and invoke a semantic fallback only on rejection. That cannot catch the demonstrated false acceptances.

### 2. Evaluate new properties against what the predecessor allowed

Existing named properties continue to recurse. For a candidate-only property, determine its predecessor constraints: its matching `patternProperties` schemas, or `additionalProperties` when neither a named property nor a pattern applies. Optional means absence is allowed, not that the value is unconstrained.

For the initial supported proof domain:

- With no matching patterns and predecessor `additionalProperties: false`, a new optional property can be admitted, subject to the remaining document checks.
- With no matching patterns and absent/true additional properties, only a candidate property constraint proven to accept every previously allowed value can pass. An unconstrained `{}` passes; a newly declared string constraint does not.
- With schema-valued additional properties and no pattern interaction, compare that predecessor subschema to the candidate property using the same sound rules.
- Candidate-only properties in objects with `patternProperties` fail closed as unproven in this change. Do not invent a regex containment or overlap algorithm.

Retain conservative property-removal rejection for now. Preserve ordinary safe required-set reduction, supported type/enum widening, and constraint removal only when the reference and keyword guards establish that the local reasoning applies.

Alternative rejected: automatically set `additionalProperties: false` during authoring. That changes submitted and existing schema semantics rather than enforcing compatibility.

### 3. Guard non-local semantics before local acceptance

Identical parsed documents remain a safe no-op, including documents with references. For a changed document containing a schema-bearing `$ref`, return an unsupported-reference proof result before applying local loosening rules. This intentionally rejects some safe changes until a separate reference-aware proof engine exists.

Traverse Draft-4 schema-bearing positions, including definitions, properties, pattern properties, items, additional-property/item schemas, combinators, and schema dependencies. Do not interpret literal `$ref` keys in `enum`, `default`, or examples as references. If a schema-bearing position cannot be classified safely, fail closed.

Do not treat `id` or `format` as universally harmless annotations. Reference scope can depend on `id`; format enforcement depends on the configured validator. The first change conservatively rejects changes to these keywords in changed schema nodes unless the relevant semantics are explicitly proven unaffected. Retain safe annotation edits such as titles and descriptions in the supported reference-free domain.

Alternative rejected: require only `$ref`, `definitions`, and combinator text equality. References can target other schema positions, and a target's loosening can become a restriction beneath `not`.

### 4. Use a bounded numeric proof domain

Keep integer values in their exact signed/unsigned representations, using checked arithmetic for cross-representation comparison and divisibility. Do not use a rounded quotient's fractional part as proof of `multipleOf` containment. Identity and removal of numeric constraints remain supported where the surrounding proof is valid.

Support changed numeric constraints only where containment can be established for the application's actual validator semantics. Mixed representations, fractional divisibility, overflow, or precision-sensitive cases that lack a sound rule return unproven rather than approximating. Do not add epsilon tolerances or a general arithmetic dependency in this change.

Regression coverage must include small supported integer loosenings, values around 2^53, signed/unsigned boundaries, decimal bounds and multiples, and exclusive-bound interactions. If a mathematical interpretation and the deployed validator disagree, preserve the actual runtime boundary and reject uncertain comparisons; changing validator semantics is a separate decision.

### 5. Bound checker work and verify rules rather than sampling acceptance

Use a shared deterministic work budget for the proof traversal: at most 64 schema levels, 10,000 schema-node visits, and 100,000 comparison steps per pair. Charge collection comparisons, including enum membership and schema equality, rather than bounding recursion alone. Budget exhaustion returns unproven; it must not silently truncate a proof into acceptance. These are proposed checker limits, not new limits on schema creation, and do not claim to bound existing pre-check parsing/compilation.

Tests may use bounded generated schemas and payloads as a counterexample detector: whenever the checker accepts and the predecessor validator accepts a sampled payload, the candidate validator must also accept it. Sampling is not an acceptance algorithm or a mathematical completeness claim. Each acceptance rule still needs a stated justification and targeted interaction tests.

Use the current Rust validator configuration without a dependency upgrade. Test both raw business validation and representative composed contracts, preserving reference scopes and format behavior.

### 6. Keep pairwise checking separate from append serialization

The handler continues to read the latest schema and compare before updating. Its proof is relative to the document it read. It does not atomically bind that read to the registry write.

Explicit deferred work item: **serialize appends per registry/schema, or introduce an equivalent conditional commit protocol, then verify two competing candidates cannot create an incompatible sequence**. This needs a separate proposal, durability assessment, and concurrency tests; no lock or AWS resource is added here.

Update cache-safety and producer documentation so it does not claim this checker-only change establishes unconditional safety under concurrent appends. Issue #30's broader acceptance work remains separate and must not reuse an unsound fast path.

## Risks / Trade-offs

- More false rejections for reference-dependent, pattern-interacting, or uncertain numeric changes -> explain unsupported proof cases and retain no-op appends; broaden only through later proven rules.
- Current tests encode unsafe optional-property acceptance -> replace those expectations with validator-backed witnesses and add a closed-object safe-addition case.
- A bounded test corpus can miss counterexamples -> combine it with explicit proof-domain guards and per-rule reasoning; never use passing samples to authorize an append.
- Existing stored versions may already contain breaking transitions -> this change does not repair historical contracts or establish deployed state.
- Pairwise correctness does not eliminate the existing concurrent-write race -> keep the separate serialization work item visible in the design and operational documentation.

## Migration Plan

No schema documents or historical versions are rewritten. Publisher appends that relied on unsafe or unproven acceptance may now receive 400; document the corrected optional-property rule and the difference between breaking and unsupported evolution. Roll out through the normal reviewed backend release process, outside this planning capture. Do not recommend reverting the hardening merely to restore unsafe acceptance.
