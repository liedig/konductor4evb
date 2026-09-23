# Publisher schema authoring in the console: register schema + append version on the producer domain page

[Documentation home](../README.md)

- Recorded date: 2026-09-02
- Change ID: `schema-authoring-ui`
- Area: Web UI
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

Zero backend changes—forms wire to the existing tested `POST /schemas` / `POST /schemas/versions` endpoints. Authoring lives on the producer domain page's schemas panel (producer concern, not the consumer-facing catalog), gated by the existing producer-catalog 403 signal rather than a new authorization check. Plain monospace textarea with a client-side `JSON.parse` guard over adding an editor dependency (Monaco/CodeMirror); the server remains the draft-4 authority. `schemaType` fixed to `JSONSchemaDraft4`; target environment displayed from the console's API stage segment, never selectable. Failure responses (400/403/409) render inline and never clear the editor.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-09-02-schema-authoring-ui/proposal.md). The recorded date above is not necessarily the archive or merge date.

[Decision index](README.md)
