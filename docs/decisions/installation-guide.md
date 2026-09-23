# Single installation doc, README becomes pointer

[Documentation home](../README.md)

- Recorded date: 2026-04-18
- Change ID: `installation-guide`
- Area: Documentation
- Status: recorded decision; read later changes before applying it.

## Decision and rationale

Created `docs/installation.md` as the single runnable end-to-end installation guide covering the three-stack deploy model (notifications, core, trust), prerequisites, smoke-test walkthrough, troubleshooting, and uninstall. Replaced the README's "Getting Started" section (which had duplicated and incomplete deploy steps) with a pointer to the installation guide. Rationale: a single doc eliminates drift between README and installation instructions; an operator following one doc from start to finish is less likely to miss a step.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-04-18-installation-guide/proposal.md). The recorded date above is not necessarily the archive or merge date.

## Current relevance

The single-entry installation journey is retained at [installation](../operations/installation.md). Detailed runbooks now have their own pages under the [documentation organization decision](documentation-organization.md).

[Decision index](README.md)
