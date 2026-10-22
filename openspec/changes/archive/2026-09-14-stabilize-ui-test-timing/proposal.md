## Why

Local full-suite UI runs failed intermittently during redeployment preparation, while the two identified failing tests passed in isolation. Missing failure traces and partially controlled timing prevent us from distinguishing a product defect from a test synchronization problem, so deployment remains paused.

## What Changes

- Preserve complete failure output and traces before rerunning tests, including the failing assertion, source revision, command, and elapsed time.
- Make toast expiry tests explicitly control browser time and verify focus or hover preconditions before advancing it.
- Synchronize long-modal measurements with visible rendering while retaining viewport bounds and reachable-action assertions.
- Reproduce the timing weaknesses with focused regression checks, then validate the affected tests repeatedly and in the complete serial suite.
- Document the timing and evidence-retention approach and record the decision. Do not change product behavior based on the unproven pointer-events hypothesis.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. Existing `web-ui` requirements already cover modal reachability and six seconds of unpaused notification display. This tooling and test change preserves those contracts; specs are explicitly skipped.

## Impact

- Primary targets: `web/tests/ui/toasts.spec.ts`, `web/tests/ui/overlays.spec.ts`, and a narrowly scoped test helper or fixture only if needed.
- Documentation: `docs/development/testing.md` and `docs/development/decisions.md`.
- Diagnostic output stays in ignored local evidence directories and contains no credentials or live user data.
- No dependency upgrades, API changes, infrastructure changes, deployment, or external writes.
- Product component changes require evidence of a failing existing contract and an updated plan before implementation.
