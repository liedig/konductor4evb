# Design: Schema Authoring UI

## Context

The console is a Nuxt SPA calling the OAuth control-plane surface through `useApi` (typed get/post/del with `ApiError` status mapping) and domain-scoped composables (`useCatalog`, `useDomains`). Forms already exist for adjacent journeys (access-grant panel, subscribe action) and follow the same pattern: inline errors, toast feedback, panel refresh after mutation. The schemas panel (`DomainSchemasPanel.vue`) already distinguishes publisher scope (producer catalog) from the 403 non-publisher view. The backend endpoints (`POST /schemas`, `POST /schemas/versions`) exist and are tested in `schema-admin`; this change is frontend-only. See proposal.md for motivation.

## Goals / Non-Goals

**Goals:**

- Self-service schema authoring for publishers from the domain page they already use
- Error handling that never loses typed content (a `400`/`409` must not clear the editor)
- Zero new backend surface

**Non-Goals:**

- Schema content diffing, versioning history UI, or a schema IDE (syntax highlighting beyond a monospace editor)
- Environment selection (stage-inferred by the API; displayed, never chosen)
- Promotion workflows (parked separately)
- IAM-surface authoring (console is OAuth-only, consistent with the rest of the UI)

## Decisions

### 1. Plain JSON textarea with client-side parse check, no editor dependency

A monospace `<textarea>` with JSON validation feedback (on blur, then live once the field has been touched) and a Format button that pretty-prints valid JSON — plus a parse guard before submit. Alternatives (Monaco, CodeMirror) add a large dependency and build complexity for two forms; the AGENTS.md "start simple" guidance wins. The server remains the authority on draft-4 validity — the client only checks parseability and formatting.

### 2. Authoring lives on the producer domain page, not the catalog

The catalog is the consumer-facing, entitlement-scoped surface; authoring is a producer concern. `DomainSchemasPanel` already renders exactly when the caller has publisher scope, so gating the affordances there reuses the existing authorization signal (the producer catalog call's 403) rather than inventing a new one. Append-version hangs off each schema row; register hangs off the panel header.

### 3. New composable methods over a new composable

Add `createSchema` and `appendSchemaVersion` to `useCatalog` (the schemas API client) rather than a new composable — same resource family, same error semantics. Types go in `app/types/api.ts` mirroring the `schema-admin` request/response models.

### 4. Environment display from runtime config

The console's API base URL (runtime config, loaded via the Nuxt plugin) determines the stage; the form derives and displays the environment label from it. If the stage cannot be derived, the form shows the raw stage string rather than failing — display is informational only.

## Risks / Trade-offs

- [Large schema content in a textarea is awkward to edit] → Accepted for v1; paste-oriented workflow (publishers author in their own editor). Upgrade path to a real editor is additive.
- [Publisher scope is inferred from the producer-catalog 403, so affordances could flicker on slow loads] → Panel already has a pending state; authoring affordances render only after the catalog resolves successfully.
- [Server error bodies may evolve when `validate-at-ingest` adds draft-4 parse validation] → The form renders `ApiError` bodies verbatim, so richer server errors appear without UI changes.
