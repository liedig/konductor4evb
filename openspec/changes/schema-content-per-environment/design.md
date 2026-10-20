# Design

## Context

`GET /schemas/{schemaName}?domain=…` returns `latestVersions` per environment plus one `content` field — the globally highest version's content, so it cannot answer "what runs in staging". The console's only content rendering lives on the consumer catalog page (`/catalog/{producer}/{schema}`), which composes the envelope contract for the caller's pinned version and requires an entitlement. The registries themselves answer per-version reads: `DescribeSchema` with `SchemaVersion` returns any version's content, and version content is immutable (correcting the promote-schema-versions spike 1.1 claim; see that change's design.md).

## Decisions

- **A lazy per-environment endpoint, not a fatter describe response.** Embedding `contentByEnvironment` in the detail response would multiply its payload by the environment count for a page whose primary job is pipeline state. A dedicated `GET /schemas/{schemaName}/content?domain=&environment=[&version=]` is fetched only when the publisher asks to see content, mirroring the catalog page's one-read-per-selection pattern.
- **The registry is the read source, not the version-content store.** The store's job is promotion provenance; it holds nothing for schemas that predate recording, while the registry holds every version immutably. Reading through the registry keeps the endpoint uniform for old and new schemas. (`resolve_source_content` in the promotion steps made the same call for the same reason.)
- **The describe-schema function serves the route.** It already owns `/schemas/{schemaName}` and `/schemas/{schemaName}/contract`, holds a Schemas client with read access to every environment's registry, and its `authorize()` already implements the publisher-binding-or-platform-role rule this endpoint reuses. The route is wired in both OpenAPI definitions to that function.
- **Authorization mirrors the detail route's producer path exactly**: no consumer context, publisher binding on the producer domain, or platform role (admin and auditor both read). No new grant action; inspection is part of reading your own catalog entry.
- **The viewer is a modal, not a new page section.** Content is an occasional deep-dive; the page's cards stay glanceable. The modal reuses `NxModal` (focus return to the opener, standard dismissal), the catalog page's pretty-print helper, and `useCopyToClipboard()` for the copy confirmation. No raw controls.
- **Version-history rows view the version, pipeline nodes view the latest.** Same endpoint both ways: the node omits `version`, the history row passes it. This keeps one code path and one contract.

## Risks

- **Registry read failures surface as 5xx on the new route.** Same failure mode as the existing describe route; accepted, and the UI contract is toast + dismissible modal.
- **Content size.** Schema documents are small (kilobytes); a per-request single-version read is bounded. No pagination or truncation needed.
