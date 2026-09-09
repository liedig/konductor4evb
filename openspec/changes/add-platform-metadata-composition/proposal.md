## Why

Konductor currently defines envelope metadata in code and validates registered schemas against business data only. Platform teams need centrally defined, consistently generated metadata that participates in event validation, with environment-scoped activation and immutable historical contracts rather than retroactive changes to past events.

## What Changes

- Introduce organisation-level, immutable metadata revisions with protected built-in defaults: `schemaVersion`, `idempotencyKey`, and `publishedAt`. Add a platform-owned numeric `metadataRevision` discriminator to new-format events.
- Allow platform administrators to add fields and nested objects using a bounded catalogue of platform generators: UUID, UTC timestamp/date, AWS region, organisation identifier, and environment. Producers cannot submit extension values. Existing field definitions, types, and generators cannot be removed or changed.
- Compose domain-owned business schemas and platform metadata into inspectable, independently versioned event contracts. Business-schema documents in existing registries remain unchanged; composed contracts are exposed separately.
- Activate a metadata revision independently per environment, consistently across domains. Newly resolved admission contracts use the active revision; in-flight admissions retain their selected revision and accepted events retain their values through delivery, retries, and replays.
- Preserve defaults and existing semantics, including the explicit caller-supplied idempotency-key exception and `publishedAt` derived from SQS `SentTimestamp`. Only the latter is deterministically materialized after queue acceptance.
- Keep metadata evolution separate from the business-schema append gate in `gate-schema-promotion`. Metadata activation does not mint business schema versions, move subscription pins, or emit a business-schema promotion event.
- Add a platform metadata management UI: protected defaults, extension editor, preview, revision diff/history, per-environment readiness and activation, and role-based read/write controls. Extend domain/catalog views with current and historical composed contracts.
- **BREAKING for closed-envelope consumers:** new-format events contain `metadataRevision` and configured extensions. Consumers that reject unknown metadata properties require an explicit readiness review before rollout or activation. Existing producer request bodies remain unchanged; legacy events and queued messages remain readable.

## Capabilities

### New Capabilities

None. Metadata is part of the existing envelope, schema-management, publishing, and web-console capabilities.

### Modified Capabilities

- `event-envelope`: Mandatory generated metadata, revision identity, preservation of built-in semantics, and historical/replay behavior.
- `schema-management`: Platform-only metadata configuration, additive revision checks, environment activation, and composed current/historical contract discovery.
- `event-publishing`: Admission-time metadata generation and validation, cutover-aware caches, immutable queued metadata, and safe failure behavior.
- `web-ui`: Metadata administration and revision activation interfaces plus composed-contract inspection in schema views.

## Impact

- Shared Rust envelope, request/queue models, generator/compiler helpers, typed errors, and authorization action declarations.
- `publish-events-admission`, `publish-events`, `describe-schema`, `list-schemas`, and new metadata administration handlers; the existing business-schema compatibility gate must remain scoped to business content.
- OAuth metadata-management routes, parity for composed-schema reads on OAuth/IAM surfaces, a dedicated metadata configuration/history table, restricted IAM access, and stream-driven lifecycle/audit emission.
- Nuxt admin navigation and pages, `Nx*`-based metadata components, typed API composables, `DomainSchemasPanel`, schema authoring preview, and catalog schema-detail views.
- Handler/behavior tests, offline composed-schema validation tests, isolated browser tests, API contracts, publishing documentation, architecture documentation, and the stream-driven side-effects decision record.

### Boundaries and Dependencies

- `gate-schema-promotion` remains a separate change: it is a business-schema compatibility gate, not an environment approval workflow. Its implementation is not assumed complete.
- `configure-customer-platform` remains a separate proposal-only capture. Metadata uses today's resolved organisation/environment/region inputs; this change does not rename resources, configure customer groups, or build general customer setup.
- No arbitrary scripts, remote schema resolution, per-domain metadata overrides, producer-supplied extension values, metadata deletion, backward activation, or business-schema migration automation.
- Historical schema lookup requires an explicit metadata revision for an exact composed contract. It never silently combines a historical business version with today's active metadata.
- Planning only: no deployment, live configuration changes, implementation, or commits are authorized by these artifacts.
