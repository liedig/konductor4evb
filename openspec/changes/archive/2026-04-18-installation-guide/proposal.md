## Why

There is no runnable, end-to-end installation guide for Nexus. The README has a minimal "Getting Started" section that assumes familiarity with the stack layout and doesn't walk through the three-stack deploy model (notifications, core, trust) or the smoke-test workflow. An operator following the current docs cannot reliably install Nexus from scratch into their own AWS environment.

## What Changes

- Add `docs/installation.md` — a complete, runnable installation guide covering prerequisites, three-stack deployment, smoke-test walkthrough, troubleshooting, and uninstall.
- Replace the README "Getting Started" section (Prerequisites, Build and Test, Deploy) with a single pointer to `docs/installation.md`.
- The guide targets a first-time operator with one platform account and at least one application account, deploying into their own AWS environment.

## Capabilities

### New Capabilities

None. This is a documentation-only change.

### Modified Capabilities

None. No spec-level behavior changes.

## Impact

- New file: `docs/installation.md`
- Modified: `README.md` (replace Getting Started section with pointer)
- No code, template, or API changes
