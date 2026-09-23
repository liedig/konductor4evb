# Repository map

[Contributor guide](README.md)

This is a directory map, not an exhaustive file tree. Follow the source links
for the current inventory.

| Location | Purpose |
|---|---|
| [`api/`](../../api/) | OAuth and IAM OpenAPI contracts |
| [`domain-management/`](../../domain-management/) | Domain handlers and lifecycle state machines |
| [`event-streams/`](../../event-streams/) | Publish, schema, and subscription handlers and workflows |
| [`control-plane/`](../../control-plane/) | Shared control-plane capabilities, including access and metadata |
| [`lib/konductor-common/`](../../lib/konductor-common/) | Shared Rust models, authorization, validation, and AWS helpers |
| [`web/`](../../web/) | Nuxt/Vue console and browser tests |
| [`scripts/`](../../scripts/) | Local development and operator helpers |
| [`openspec/specs/`](../../openspec/specs/) | Capability requirements |
| [`openspec/changes/`](../../openspec/changes/) | Active changes and archived design evidence |
| [`docs/`](../README.md) | Reader-facing documentation |

## Infrastructure entry points

- [Core stack](../../konductor-core-stack.yaml)
- [Optional notification identities](../../notifications-stack.yaml)
- [Domain-account trust](../../trust-stack.yaml)
- [Separately authorized organization guardrails](../../guardrail-stack.yaml)
- [Makefile](../../Makefile) for local targets

Use [installation](../operations/installation.md) for the deployment journey.
The previous full file tree remains only in the
[historical baseline](../history/documentation-baseline.md#project-structure).
