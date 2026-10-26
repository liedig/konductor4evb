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

The proof covers only the predecessor that the handler read. Serializing competing appends remains outside this guarantee. See [Publish validation](publish-validation.md) and the [Decision log](../decisions/README.md).

## Authoring environment and promotion

Writes land only in the first configured environment: the API stage selects the environment, and the schema-admin handler rejects create/append on any later stage with `400` before touching the registry. Every committed version is also recorded (canonical content plus SHA-256 hash) in the `konductor-schema-versions` table, because the registry serves only the latest version—replay and divergence detection read the store, not the registry history.

Later environments receive a schema through explicit promotion (`POST /schemas/promotions`). The workflow verifies that the target's recorded prefix matches the source and replays the missing versions one at a time with identical version numbers. It emits `SchemaVersionPublished` for the target environment, then re-pins the target's subscription rules with the widen/drain/narrow sequence described in [subscription materialization](subscriptions.md#re-pinning-widen-drain-narrow). An append in the authoring environment advances that environment's pins through the same sequence, triggered by the subscription extender off the publish event.

The schema-admin emission of `SchemaVersionPublished` stays handler-side and names the authoring environment; replayed versions announce from the workflow, never from the replay steps themselves.

## Rationale and limits

Read the [compatibility hardening decision](../decisions/harden-schema-compatibility-checker.md) before changing the proof. It preserves the counterexamples, fail-closed rules, proof budgets, and pairwise concurrency limit. Use [publish validation](publish-validation.md) for cache implications.
