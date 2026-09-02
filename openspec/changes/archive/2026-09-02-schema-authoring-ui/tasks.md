# Tasks: Schema Authoring UI

## 1. API client and types

- [x] 1.1 Add `CreateSchemaRequest`/`AppendSchemaVersionRequest` and response types to `web/app/types/api.ts` mirroring the `schema-admin` models; verify `npx nuxi typecheck` passes
- [x] 1.2 Add `createSchema` and `appendSchemaVersion` to `useCatalog` posting to `/schemas` and `/schemas/versions`; verify types align with `useApi`'s `post` signature and `npx nuxi typecheck` passes

## 2. Authoring form component

- [x] 2.1 Create a schema-content form component (event-name input for create, monospace content textarea, client-side `JSON.parse` guard, inline error display, submit state) following the existing access-grant form patterns; verify the parse guard blocks submission without an API call
- [x] 2.2 Surface `ApiError` statuses inline (`400` with server detail, `403`, `409`) without clearing form state, and push success feedback via `useToast`; verify by exercising each status against the dev stage or a mocked `useApi`
- [x] 2.3 Display the target environment derived from the console's runtime config (no selector); verify the label matches the configured API stage
- [x] 2.4 Add JSON validation feedback (on blur, then live) and a Format affordance to the content field; verify invalid JSON shows an inline error without an API call and Format pretty-prints valid JSON

## 3. Domain page integration

- [x] 3.1 Add the "Register schema" action and form to `DomainSchemasPanel`, rendered only after the producer-scope catalog resolves (never on the 403 view); verify a non-publisher session renders no authoring affordances
- [x] 3.2 Add the per-schema "append version" action wired to the same form; verify success refreshes the panel badges with the new version
- [x] 3.3 Wire successful mutations to re-fetch the producer catalog so the schema list and version badges reflect the write; verify no stale entries after create/append

## 4. Docs and verification

- [x] 4.1 Update `docs/development/decisions.md` with the textarea-over-editor and panel-placement decisions; verify the entry follows the existing format
- [x] 4.2 Run the web lint/type gates (`pnpm lint`, `nuxi typecheck`) and verify both pass
- [x] 4.3 Smoke-test against the dev stage: register a schema, append a version, and confirm both appear with correct badges; verify the registered schema is visible via `GET /schemas?domain=...`
