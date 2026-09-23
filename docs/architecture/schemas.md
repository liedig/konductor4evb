# Schema management and compatibility

[Documentation home](../README.md)

Business schemas are separate from [platform metadata](metadata.md). This page explains the schema-write boundary.

## Schema management (append-only)

Publishers manage schemas through two synchronous endpoints:

- `POST /schemas` creates `{namespaceIdentity}@{eventName}` as version 1 in the stage-selected registry, like `CreateSchema`. Duplicate creation returns `409`.
- `POST /schemas/versions` appends a version, like `UpdateSchema`. Earlier versions remain immutable. A missing schema returns `404`.

Both endpoints require an `ACTIVE` domain and a `publisher` binding for the `schema:create` or `schema:version-append` action. The API stage selects the environment.

Schemas must use JSON Schema draft-4. The endpoints reject other `schemaType` values and content that cannot compile as draft-4 with `400`, before writing to the registry.

Schemas are append-only. No delete, retire, replace, or version-deletion operation exists. Publisher permissions cover only `CreateSchema` and `UpdateSchema` inside organization-namespaced registries. Platform administrators retain registry lifecycle and policy control.

Every append passes the pairwise compatibility gate in `konductor-common` (`schema_compatibility`). It must prove that the candidate accepts every payload accepted by the latest version it read. It returns `400` for restrictions, unsupported constructs, inexact numeric comparisons, or exhausted proof budgets. No role or flag bypasses the gate.

The proof covers only the predecessor that the handler read. Serializing competing appends remains outside this guarantee. See [Publish validation at ingest](publish-validation.md) and the [Decision log](../decisions/README.md).

## Rationale and limits

Read the [compatibility hardening decision](../decisions/harden-schema-compatibility-checker.md) before changing the proof. It preserves the counterexamples, fail-closed rules, proof budgets, and pairwise concurrency limit. Use [publish validation](publish-validation.md) for cache implications.
