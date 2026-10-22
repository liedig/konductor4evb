## 1. Preserve and classify baseline evidence

- [x] 1.1 Run the unchanged complete UI suite with a unique ignored evidence directory and unfiltered output; verify the retained record includes revision, exact command, elapsed time, and actual exit status, with traces for any failures.
- [x] 1.2 Inspect any modal and toast failure at its exact assertion or action; record the observed DOM, focus, timing, and actionability evidence, or explicitly record non-reproduction. If runtimes are abnormal, attach contemporaneous local resource and server evidence without asserting an unsupported cause.

## 2. Stabilize notification timing

- [x] 2.1 Demonstrate the uncontrolled-clock weakness with a bounded focused check that separates host delay from simulated lifetime; retain before/after evidence and distinguish this reproduction from the unproven historical failure.
- [x] 2.2 Install the clock before navigation for clock-driven toast tests, pause after page readiness and before notifications, and advance time explicitly; verify before-expiry visibility and after-expiry removal remain correct despite host-side delay.
- [x] 2.3 Assert visible toast and focus/hover preconditions before paused intervals, retaining the focus-preserving synthetic click for notifications added while paused; verify focus, hover, window inactivity, overlapping pauses, and full lifetime on resume all pass.

## 3. Synchronize modal measurement

- [x] 3.1 Wait for visible dialog rendering before reading long-modal bounds, adding retrying geometry checks only if evidence requires them; verify non-null bounds fit the 390-by-600 viewport and Finish reading remains actionable and closes the dialog without a forced click.

## 4. Document and validate

- [x] 4.1 Update `docs/development/testing.md` with explicit clock control, interaction preconditions, and per-run evidence retention; verify its commands preserve real exit status and do not overwrite previous evidence.
- [x] 4.2 Record the confirmed timing findings, chosen synchronization approach, and remaining uncertainty in `docs/development/decisions.md`; verify the entry does not claim an unproven product defect or deployed baseline.
- [x] 4.3 Run both affected spec files three consecutive times with one worker and no retries; verify all assertions pass and keep each invocation's output separately.
- [x] 4.4 Run the complete serial UI suite twice with separate retained output, then `pnpm lint`, `pnpm typecheck`, and `pnpm build` from `web/`; report exact results and runtimes, and investigate failures before declaring completion.
- [x] 4.5 Review the final diff and validate this OpenSpec change; verify assertions were not weakened, no unrelated or product-code changes slipped in, and no deployment occurred. If corrected synchronization exposes a product defect, preserve its reproduction and revise the plan before implementing a component fix.

## Validation evidence (2026-09-14)

Evidence is local under ignored `web/ui-evidence/stabilize-ui-test-timing/`. Wall times include CLI startup.

| Check | Result | Evidence directory |
| --- | --- | --- |
| Unchanged baseline | 57/57, exit 0, 106s | `baseline.2PGNqV` |
| Clock diagnostic before fix | Expected visibility failure, exit 1; 350ms host delay advanced browser time 354ms | `clock-red.8Ahane` |
| Clock diagnostic plus affected specs after fix | 12/12, exit 0, Playwright 42.8s; same delay advanced browser time 0ms | `clock-green.KwuEuL` |
| Focused run 1 | 11/11, exit 0, 41s | `focused-1.ElZHPl` |
| Focused run 2 | 11/11, exit 0, 42s | `focused-2.OXt4iK` |
| Focused run 3 | 11/11, exit 0, 43s | `focused-3.9MehVt` |
| Full run 1 | 57/57, exit 0, 101s | `full-1.z8p7v6` |
| Full run 2 | 57/57, exit 0, 103s | `full-2.4YHRww` |
| Lint | exit 0, 6s | `lint.BkPv5t` |
| Typecheck | exit 0, 8s | `typecheck.wVsfdo` |
| Production build | exit 0, 8s | `build.QyfAVz` |

The UI reviewer found no actionable issues in the changed test files. The temporary diagnostic is preserved with its run evidence, not shipped. Historical failures and abnormal runtimes did not reproduce; no product defect or deployed revision is inferred. No product components, dependencies, backend code, or infrastructure changed, and no deployment occurred.
