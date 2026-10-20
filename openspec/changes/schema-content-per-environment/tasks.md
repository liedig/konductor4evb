# Tasks

## 1. API: per-environment content route

- [x] 1.1 Add `GET /schemas/{schemaName}/content` to both `api/openapi-oauth.yaml` and `api/openapi-iam.yaml`, wired to the describe-schema function, with `domain` (required), `environment` (required), and `version` (optional integer) query parameters, the `publisher`-or-platform authorization note, and `200`/`403`/`404` responses. Verify with the api_surfaces contract test (`cargo test -p konductor-common`).
- [x] 1.2 Implement the route in `event-streams/functions/describe-schema/src/handler.rs`: resolve the environment's registry via the existing namespace resolver, read the requested version (or the environment's latest when omitted) with `DescribeSchema` + `SchemaVersion`, map NotFoundException to `404`, reuse `authorize()` unchanged (publisher binding on the producer domain or platform role, no consumer context). Response: `{ schemaName, environment, version, content }`. Verify with `cargo test -p describe-schema` covering: latest-by-default, explicit historical version, schema absent from the environment (404), unknown version (404), no binding (403), IAM-surface parity.

## 2. Console: content viewer

- [x] 2.1 Add the typed client call (`getSchemaContent`) to `useCatalog.ts` and `web/app/types/api.ts`, and the mock mode to `web/tests/ui/api.ts`. Verify with `pnpm typecheck`.
- [x] 2.2 Build the viewer: a component (or page-local block) in `web/app/` using `NxModal`, titled `{environment} · v{version}`, pretty-printed content (reuse the catalog page's pretty-print approach), a copy action via `useCopyToClipboard()` confirming inline, and a loading then error-toast path that leaves the modal dismissible. Verify with `pnpm lint` and `pnpm typecheck`.
- [x] 2.3 Wire the affordances on `pages/domains/[name]/schemas/[schema].vue`: each pipeline node with a version (latest, no `version` param) and each environment badge on version-history rows (explicit version). Nodes whose environment lacks the schema render no affordance. Verify the modal restores focus to the opener.
- [x] 2.4 Add Playwright coverage in `web/tests/ui/schema-promotion.spec.ts` (or a new spec): publisher inspects staging from the pipeline, inspects a historical version from version history, copy confirms inline, absent-environment node has no affordance, auditor read-only, load failure toasts. Verify with `rtk playwright test`.

## 3. Documentation and review

- [x] 3.1 Add the endpoint to the API reference docs (both surfaces) and a CHANGELOG entry under `Unreleased`. Update `openspec/specs` only via archive sync. *(The API reference, `docs/reference/api.md`, defers to the OpenAPI files by policy — updating them in 1.1 is the reference update.)*
- [x] 3.2 Run `make check`, `rtk cargo test --workspace`, the `rust-reviewer` droid on the backend diff, and the `ui-reviewer` droid on the console diff. Verify clean.
- [x] 3.3 Deploy to the dev installation (`make deploy-core`) and spot-check live: view a pre-recording schema's v1 in staging and a current schema's latest in prod from the console. Record results in a verification note.
