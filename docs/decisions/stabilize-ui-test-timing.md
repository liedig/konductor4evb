# Controlled UI test timing

[Documentation home](../README.md)

- Recorded date: 2026-09-14
- Change ID: `stabilize-ui-test-timing`
- Status: recorded decision; see the linked change for scope and validation.

## Decision and rationale

Toast expiry tests now install the browser clock before navigation and explicitly pause it after gallery readiness, before creating notifications. Installing the clock alone leaves it running: a bounded diagnostic advanced 5900ms, delayed the host assertion by 350ms, and observed another 354ms of browser time and an expired toast. With explicit pause, the same host delay advanced browser time by 0ms; the toast stayed visible until a further 200ms of simulated time. Announcement rendering time is included in the lifetime budget.

Tests assert notification focus/hover preconditions before advancing paused intervals, preserve the synthetic click used to add a toast without moving focus, and wait for long-modal visibility before measuring bounds. Existing six-second lifetime, pause-union, viewport, and actionability contracts remain unchanged. No Reka or product component patch was justified.

The unchanged baseline passed 57/57 in 106 seconds. Earlier full-suite modal/toast failures and 22.9/33-minute runtimes did not reproduce, and their original traces had been replaced by a later passing run. The diagnostic proves a test-clock weakness, not the cause of those historical failures. Investigation runs now use separate ignored evidence directories and retain unfiltered output and actual exit status. This change makes no claim about the deployed backend revision and performs no deployment.

## Evidence

[OpenSpec change](../../openspec/changes/archive/2026-09-14-stabilize-ui-test-timing/proposal.md).

[Decision index](README.md)
