## Context

See `proposal.md` for motivation. The serial Playwright harness uses a fresh browser context per test and a shared local Nuxt dev server. It retains traces on failure, but subsequent runs clear the default output directory. The last isolated rerun passed both identified tests and replaced the earlier failure evidence.

`toasts.spec.ts` installs the browser clock after navigation without pausing it. Assertions made after advancing 5,900 ms therefore compete with real elapsed time before a six-second expiry. The added-while-paused case also presses F8 without asserting focus before advancing time. `overlays.spec.ts` takes a one-shot bounding box after clicking the long-modal opener without first asserting dialog visibility.

The app owns toast lifetime through `useToastLifetime`; Reka receives an infinite duration. Its pause predicate combines hover, region focus, and window inactivity. The reviewed layer code does not establish a product bug. Full-suite runtimes of 22.9 and 33 minutes are observations, not evidence of a particular resource or sleep problem.

## Goals / Non-Goals

**Goals:**

- Separate controlled browser time, observable UI readiness, and host execution time.
- Preserve the existing interaction contracts and assertions while removing uncontrolled timing dependencies.
- Make every validation failure inspectable before another invocation replaces its artifacts.

**Non-Goals:**

- No arbitrary sleeps, widened expiry tolerances, automatic retries, increased global timeouts, or parallelism changes to conceal failures.
- No speculative Reka patch, dependency upgrade, or product lifetime change.
- No deployment or changes to parked work.

## Decisions

### 1. Preserve evidence before interpreting a failure

Run the baseline with complete output and a unique ignored output directory under `web/ui-evidence/stabilize-ui-test-timing/`. Preserve the actual process exit status, source revision, command, run duration, and failure traces. Do not filter away assertion stacks or rely on a pipeline's final command status. Use separate directories for subsequent invocations.

Inspect the exact failing action, DOM snapshot, and call log. For toast failures, distinguish initial appearance, paused survival, near-expiry visibility, and final removal. For modal failures, distinguish absent bounds, invalid dimensions, intercepted actions, and failure to close.

Alternative rejected: repeatedly rerun the full suite with filtered output. It loses evidence and does not diagnose intermittent failures. If the baseline passes, record non-reproduction and continue with focused timing checks without claiming the original cause is proven.

### 2. Install and explicitly pause the toast clock

For clock-driven tests, install the clock before navigation, let startup complete, then pause at a deterministic test time before creating notifications. Advance only through explicit clock operations. Keep non-clock tests on ordinary browser time; introduce only a small local helper if several tests need identical setup.

Assert that the initial toast is visible and that F8 actually focuses the notification viewport before advancing a paused interval. Verify hover preconditions where relevant. Keep the synthetic Information click in the added-while-paused case because it intentionally preserves focus in the notification region.

Check just-before-expiry visibility and after-expiry removal against controlled time, including time already consumed before a pause. Demonstrate that host-side assertion latency does not consume simulated lifetime. An explicit delayed assertion may be used as a bounded diagnostic, not as a synchronization mechanism in shipped tests.

Alternative rejected: add time margins or retries. Those weaken the lifetime contract rather than isolate it.

### 3. Observe modal readiness before measuring

Wait for the named dialog to be visible before taking bounds. Keep non-null bounds, width and height limits, and an ordinary actionability-checked click on Finish reading. If evidence shows unstable geometry after visibility, use a retrying geometry assertion tied to the expected viewport bounds, not a fixed delay or forced click.

Alternative rejected: change modal CSS or pointer-events cleanup based only on the test name. Those changes need evidence of an actual product defect.

### 4. Validate both synchronization and product behavior

Use focused red/green checks for demonstrated timing weaknesses. Run the affected files three consecutive times with one worker and no retries, then run the complete suite twice with separately retained output. Also run lint, typecheck, and the production build.

If a product-contract failure survives corrected synchronization, stop treating it as test-only work: preserve a minimal reproduction and revise the plan before changing components. If abnormal runtimes recur, inspect run-specific server logs and contemporaneous resource/sleep evidence rather than assuming a library defect.

## Risks / Trade-offs

- Paused timers can stall application initialization -> install before navigation but pause only after observable page readiness; verify any deferred Reka work through explicit clock advancement.
- A readiness assertion might hide an interaction defect -> retain normal click actionability, focus checks, viewport bounds, and dismissal assertions.
- Passing isolated checks may miss suite-level pressure -> require complete serial-suite validation and report elapsed times.
- Failure artifacts may contain sensitive data -> use only isolated mocked pages and keep artifacts local and ignored.
- Original failures may not reproduce -> distinguish proven test weaknesses from unresolved historical failures in the validation report.

## Migration Plan

No runtime migration or deployment is required. Land test and documentation changes through the normal review process. Revert those changes if necessary without altering production behavior. Redeployment readiness is a separate decision after validation and verification of the actual deployed backend state.
