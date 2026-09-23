# Product renamed to "Konductor for Amazon EventBridge"; repo renamed to `konductor4evb`

[Documentation home](../README.md)

- Recorded date: 2026-08-12
- Change ID: `rename-to-konductor`
- Area: Cross-cutting
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

**Why**: the former name (see git history) collided with an internal EventBridge feature code name and wrongly implied the platform is an EventBridge product.

**Strategy**: flag day—nothing deployed was worth keeping, so every identifier was renamed in one pass (crate `konductor-common`, stack `konductor-core`, bus `konductor-bus`, event sources `konductor.domain-management`/`konductor.event-streams`, trust role `{org}-konductor-trust-role`, env vars `KONDUKTOR_*`) with no compatibility shims or migration; the dev stacks were deleted and redeployed, and the full request → approve → provision round-trip was re-verified live. A follow-up sweep removed the old name absolutely everywhere, including archived OpenSpec change history.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-08-12-rename-to-konductor/proposal.md). The recorded date above is not necessarily the archive or merge date.

[Decision index](README.md)
