# SDK test features belong in development dependencies

[Decision index](README.md)

- Recorded from merge: 2026-09-18, `7cbc202` (PR #39).
- Status: implemented dependency-scope change.
- Evidence: [workspace manifest](../../Cargo.toml) and member development dependencies,
  including [konductor-common](../../lib/konductor-common/Cargo.toml).

## Decision and rationale

Normal workspace SDK dependencies no longer enable `test-util`. Members that
need SDK mocks explicitly enable it through development dependencies. Cargo
resolver 2 separates those feature needs from normal production builds.

This keeps test support available without making it a default production
feature. SDK versions were not changed by this refactor. Removing dependencies
or upgrading them was not needed to address the feature scope.

## Limits

Production checks and test builds are different workloads. Their output sizes
are not a matched before/after benchmark and do not establish disk savings.
The old [testing examples and estimates](../reference/documentation-discrepancies.md#d04)
remain flagged for a separate correction.
