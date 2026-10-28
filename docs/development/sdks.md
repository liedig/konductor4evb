# SDK generation

[Documentation home](../README.md)

Konductor language SDKs are generated from a projection of the maintained API
contracts, then wrapped in a small handwritten public interface per language.
This page is the contributor reference for that pipeline. Using an SDK is
covered by the application-team guides; the route inventory itself lives in
[API contracts](../reference/api.md) — do not copy it here.

## Contract ownership

The [OAuth and IAM OpenAPI files](../reference/api.md) are the only maintained
contracts. SDKs target the OAuth (Cognito-authorized) surface, so they
generate from `api/openapi-oauth.yaml` only; the IAM surface is not an SDK
input. SDKs support exactly four operations — `PublishEvent`, `Subscribe`,
`Unsubscribe`, and `DescribeSubscription` — and deliberately exclude platform
administration routes. A contract change to a covered operation is a contract
change for every SDK; a platform-only route addition changes no SDK.

`sdks/codegen/extractor.py` builds the projection. It selects the four
operation IDs from the OAuth contract, collects their transitive local
`#/components/...` references, and fails on a missing operation or a
dangling reference.
Contract defects are recorded in the
[discrepancy register](../reference/documentation-discrepancies.md) and
corrected in the OpenAPI files before generation — never patched over in
generator configuration.

## Permitted projection normalizations

When producing the projection, the extractor:

- Strips authentication (`security`) and AWS deployment integration
  (`x-amazon-apigateway-*`, `servers`). The handwritten client supplies
  authentication, and integration details say nothing about SDK shape.
- Apply output-only normalizations — for example, representing response
  status enums as plain strings for forward compatibility. Each such
  normalization must be named, tested, and confined to the projection.

The projection must not:

- Weaken request-side validation or constraints from the maintained contract.
- Invent response fields that are absent from the maintained contracts.
- Copy unrelated routes or deployment-specific endpoint values into the SDK.

## Build and checks

```sh
make sdk-check   # drift check, ruff lint/format, mypy (strict, generated code excluded)
make sdk-test    # SDK tests plus codegen tests; fully mocked, no network or AWS
make sdk-build   # wheel and sdist into sdks/python/dist/
```

The package builds with hatchling from `sdks/python/pyproject.toml`; building
or installing it never needs the generator or a JVM, because the generated
client source is committed. The `[build-system]` pins hatchling to an exact
version because isolated `uv build` runs ignore `uv.lock`; bump it
deliberately. `sdks/python/LICENSE` and `NOTICE` are copies of the
repository-root files, shipped in both distributions; the distribution tests
fail if they drift. Lint, format, and type checks exclude`src/konductor/_generated/` — fix the contract or projection instead.

## Regeneration

Generated client source is committed so reviewers see the actual client
changes and package consumers need no generator or JVM. Regenerate after any
covered contract change:

```sh
make sdk-generate        # rewrite sdks/codegen/projection.yaml and sdks/python/src/konductor/_generated/
make sdk-generate-check  # non-mutating: fail on stale artifacts or nondeterministic output
```

Both use the checksum-pinned OpenAPI Generator release recorded in
`sdks/codegen/openapi-generator-pin.json` (downloaded on first use; a JVM is
a generation-time dependency only). Editing generated source by hand is never
the fix — fix the contract or the projection. The accepted generator
configuration and gate evidence are recorded in the
[Python SDK decision](../decisions/add-python-sdk.md).
