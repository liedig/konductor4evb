## Why

The root `README.md` is the first page most readers see, and its overview has fallen behind three shipped changes: explicit schema promotion with first-environment authoring, validation at publish admission, and domain system events. Two of its "Current Scope" bullets now state the opposite of shipped behavior ("no schema promotion", "business payloads are not validated"), and the discrepancy register's D08 entry claims the second one was already removed when it was not.

## What Changes

- Refresh the `## How It Works` text-art diagram in place, keeping its control-plane-over-two-columns layout and adding a System events box. The diagram reflects first-environment schema authoring with compatibility-gated appends and promotion, validation before the `202`, re-pinning on promotion, console-or-API approval, per-environment domain accounts, and opt-in system events. Box edges and connectors are realigned.
- Replace the uncommitted Mermaid draft currently in the working copy of `README.md` with the refreshed text-art diagram (the maintainer chose to keep the existing diagram style for now).
- Correct the two paragraphs under the diagram: per-environment accounts, and stages derived from the configured environment list (default `dev`, `staging`, `prod`) with schema writes accepted only on the first environment's stage.
- Correct stale claims elsewhere in the README:
  - "Why Konductor": schemas are authored in the first environment and promoted, not "versioned per environment".
  - "Core Capabilities": approval happens in the console or API (email only notifies), publishing validates the payload before the `202` (`422` on violations), schema writes target the first environment with promotion onward, subscriptions re-pin on promotion, and a new System events bullet.
  - "Current Scope": remove the false promotion and payload-validation bullets, and link the domain-teardown bullet to the known limitation recorded as D24.
- Update D08 in `docs/reference/documentation-discrepancies.md` to record that the README payload-validation claim is resolved by this change, keeping D08's other open items open.
- Fix the `docs/README.md` sentence that says the changelog lists "each release" (the changelog states it is not a release history).
- Add an `Unreleased` CHANGELOG entry.

No application, API, infrastructure, or console behavior changes.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This is a documentation correction that brings the README in line with requirements already recorded in `schema-management`, `event-publishing`, `event-subscriptions`, `domain-management`, and `domain-system-events`. No requirement changes, so specs are explicitly skipped (`skip_specs: true`).

## Impact

- Files: `README.md`, `docs/README.md`, `docs/reference/documentation-discrepancies.md`, `CHANGELOG.md`.
- The root `README.md` is not part of the MkDocs site; `docs/README.md` and the discrepancy register are, so the strict site build must stay green.
- The working copy of `README.md` holds an uncommitted Mermaid draft of the diagram; this change replaces it.
- Out of scope: the Mermaid diagram in `docs/architecture/README.md`, converting the README diagram to Mermaid, the "12+ resources" claim (kept as historical context per D08), and D08's other open items (old project tree, platform-boundaries topology, quota and uninstall inventories).
