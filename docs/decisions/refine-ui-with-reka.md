# UI primitive refinement

[Documentation home](../README.md)

- Recorded date: 2026-09-08
- Change ID: `refine-ui-with-reka`
- Status: recorded decision; see the linked change for scope and validation.

## Decision and rationale

Reka UI 2.10.4 supplies unstyled dialogs, alert dialogs, and toast rendering behind stable `Nx*` interfaces. The existing dark layout, native form controls, and semantic status mapping remain Konductor-owned. Programmatic dialog openers require explicit focus capture/restoration; confirmations remain caller-controlled and block dismissal while busy.

Notification lifetime has one local owner per rendered item, with Reka expiry disabled. A browser regression demonstrated that Reka resumes its timer on focus-out even while hovered; the user approved local timer ownership to satisfy the union-of-pause-conditions contract. The queue itself has no timer. Playwright tests cover real gallery components with non-secret config interception, not live account operations.

No handler write paths, backend side effects, resource topology, API contracts, or auth policies change. The stream-driven durability rubric therefore has no new backend paths to classify. Screen-reader and browser-zoom review remain separate acceptance checks from automated DOM/keyboard testing.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-09-08-refine-ui-with-reka/proposal.md).

[Decision index](README.md)
