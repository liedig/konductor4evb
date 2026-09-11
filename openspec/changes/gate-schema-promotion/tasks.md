## 1. Compatibility checker (`konductor-common`)

- [x] 1.1 Add `schema_compatibility` module with `check_compatible(predecessor: &Value, candidate: &Value) -> Vec<Incompatibility>` (each carrying instance path + violated rule); verify `cargo build -p konductor-common` succeeds
- [x] 1.2 Test-first the `required`/`properties` rules: newly required property rejects, dropped `required` entry passes, removed declared property rejects, new optional property passes, nested `properties` recurse — verify the new unit tests pass with `rtk cargo test -p konductor-common schema_compatibility`
- [x] 1.3 Test-first the scalar-constraint rules: `type` set widening, `enum` superset, `minimum`/`maximum`/`exclusiveMinimum` under draft-4 boolean semantics, `minLength`/`maxLength`, `minItems`/`maxItems`, `multipleOf` containment — verify the unit tests pass
- [x] 1.4 Test-first the conservative rules: `pattern` identical-or-removed, `additionalProperties` true→false rejects / false→true passes, combinator (`allOf`/`anyOf`/`oneOf`/`not`) and `$ref` subtrees deep-equal or reject — verify the unit tests pass
- [x] 1.5 Verify `rtk cargo clippy -p konductor-common --all-targets` reports no issues and the checker module stays within the file-size conventions in AGENTS.md

## 2. Gate wiring (`schema-admin`)

- [x] 2.1 In `append_schema_version`, after `check_draft4_document` and `prepare`, fetch the current latest version with a version-less `DescribeSchema` on the target registry, mapping `NotFoundException` to the established 404 (keeping the `UpdateSchema` NotFound mapping as defense); verify existing append tests still pass with `rtk cargo test -p schema-admin`
- [x] 2.2 Run `check_compatible` against the fetched document and reject tightenings with `400 Bad Request` listing offending paths (capped like `MAX_REPORTED_VIOLATIONS`); verify with handler tests: tightening-required → 400 naming the path, removed property → 400, additive append → 201, compatibility judged against latest only
- [x] 2.3 Add the `SchemaPromotionRejected` metric to the rejection log line; verify the field appears in the handler test's tracing assertions (or a targeted log capture)

## 3. Infrastructure

- [ ] 3.1 Add `schemas:DescribeSchema` to the `SchemaAdminFunction` policy in `konductor-core-stack.yaml`, scoped to `${Organization}-*` registries/schemas like the sibling statements; verify `cfn-lint` (or the repo's template check in `make check`) passes

## 4. API and user documentation

- [ ] 4.1 Document the `400` compatibility rejection on `POST /schemas/versions` in `api/openapi-oauth.yaml` and `api/openapi-iam.yaml`; verify the api-surface parity tests (`rtk cargo test -p konductor-common api_surfaces`) pass
- [ ] 4.2 Update `docs/development/publish-validation.md` (stale-acceptance now cites append-time enforcement instead of a parked follow-up) and `docs/publishing-events.md` (producer-facing evolution rules, including the new-event-name route for intentional breaks); verify the sections read consistently with the spec delta
- [x] 4.3 File the GitHub issue tracking semantic subset checking as the post-v1 refinement; verify the issue links the change and is labeled per `.github/labels.yml` — filed as [#30](https://github.com/sliedig/konductor4evb/issues/30) at proposal time

## 5. End-to-end validation

- [ ] 5.1 Run `rtk cargo test -p schema-admin -p konductor-common` and `rtk cargo clippy --workspace -- -D warnings`; verify all green
- [ ] 5.2 Run `openspec validate gate-schema-promotion --strict` (with the repo's openspec invocation); verify the change validates
