# Refuse inexact multipleOf divisors at authoring

[Documentation home](../README.md)

- Recorded date: 2026-10-22
- Change ID: none (found during the `jsonschema` 0.37 to 0.59 upgrade)
- Status: recorded decision; validated by the tests named below.

## Decision and rationale

Schema creation (`POST /schemas`) and version append (`POST /schemas/versions`) reject any `multipleOf` divisor above 9007199254740991 (2^53 - 1) with `400 Bad Request`. The message names the keyword, its JSON-pointer location (for example `#/properties/ledger/properties/units`), and the bound.

The validator decides divisibility through `f64` once a divisor exceeds 2^53 - 1. It never decided these cases exactly: the 0.37 validator accepted 74 of 316 sampled non-multiples, and 0.59 accepts 5 and rejects true multiples. The earlier regression test that claimed exactness above 2^53 passed only on its two chosen values. Admission must not stamp a version onto a payload the schema author did not intend to allow, so the platform refuses the divisor rather than validating it approximately. Divisors up to the bound, and fractional divisors such as `0.5`, are unaffected.

The check walks every position that can hold a subschema, including unknown keywords, because a `$ref` can target them. It skips `enum` and `default` values, which are instances, and treats the keys of `properties`, `patternProperties`, `definitions`, and `dependencies` as names, so a property called `multipleOf` is still allowed.

The check runs only at authoring. Publish-time compilation does not apply it, so a document stored before this gate existed keeps validating instead of turning every publish into `503`. The compatibility checker's exact divisor comparison ([compatibility hardening](harden-schema-compatibility-checker.md)) is unchanged; it reasons about documents, not about the validator's arithmetic.

## Evidence

- `schema_validation::tests::integer_multiple_of_divisors_above_the_exact_bound_are_rejected_wherever_they_appear` and `exact_multiple_of_divisors_and_non_schema_values_are_not_rejected` in `lib/konductor-common/src/schema_validation.rs`
- `integer_multiples_compare_exactly_up_to_the_largest_safe_divisor` and `integer_multiple_of_divisors_above_two_pow_53_minus_1_are_refused_at_authoring` in `lib/konductor-common/src/schema_compatibility/regression_tests.rs`
- `create_schema_with_an_inexact_multiple_of_divisor_returns_400` and `append_version_with_an_inexact_multiple_of_divisor_returns_400` in `event-streams/functions/schema-admin/src/handler.rs`

[Decision index](README.md)
