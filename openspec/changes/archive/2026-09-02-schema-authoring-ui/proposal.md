# Proposal: Schema Authoring UI

## Why

Publishers can create schemas and append versions only through raw API calls (`POST /schemas`, `POST /schemas/versions`) — the console's schema surface is read-only (`DomainSchemasPanel` lists; the catalog browses). Getting even a handful of schemas into the system requires curl and a token, which blocks realistic development of everything downstream (validation, subscriptions, notifications) and leaves publishers without a self-service authoring path.

## What Changes

- **Register-schema journey**: the producer domain page's schemas panel gains a "Register schema" form (event name + JSON Schema draft-4 content) for callers with a `publisher` binding on that domain. `schemaType` is fixed to `JSONSchemaDraft4`; the form displays the target environment derived from the console's API stage (environment is never user-selectable). Success shows the created schema name and version 1; duplicate (`409`), forbidden (`403`), and validation (`400`) failures are surfaced inline without losing the form's content.
- **New-version journey**: each schema in the producer's panel gains an "append version" action with the same content editor; success shows the new version number and refreshes the per-environment version badges.
- **Client-side guards**: content must parse as JSON before submission; server-side validation errors are displayed verbatim.
- No backend changes: both endpoints already exist, are grant-engine authorized, and are tested. The console simply wires forms to them.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `web-ui`: adds the schema authoring journeys (register, append version) to the producer domain page, alongside the existing read-only catalog and subscription journeys.

## Impact

- **Code**: `web/app` — new/extended components on the domain page schemas panel and an authoring form component; `useCatalog` (or a new composable) gains `createSchema` / `appendSchemaVersion`; `app/types/api.ts` gains the request/response types.
- **APIs**: none — consumes existing `POST /schemas` and `POST /schemas/versions` on the OAuth surface.
- **Interplay**: independent of `validate-at-ingest`, which adds server-side draft-4 parse validation (`400`) — the form's error display is built to surface exactly those responses, but nothing here depends on that change landing first.
