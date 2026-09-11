## 1. Dependency and browser baseline

- [x] 1.1 Add `reka-ui` and development-only `@playwright/test` using the pinned pnpm version; verify installation and `pnpm typecheck`, and record the installed Reka version used for the adapters.
- [x] 1.2 Add a `test:ui` script and gallery-focused Playwright configuration using local non-secret runtime-config interception and blocked external traffic; verify a gallery smoke test passes without Cognito credentials or production auth changes.
- [x] 1.3 Capture baseline gallery screenshots at 390px and 1440px widths and record current web lint/typecheck/build results; verify evidence is local and contains no session or user data, and record pre-existing failures separately.

## 2. Visual system and owned controls

- [x] 2.1 Refine semantic tokens and shared typography, spacing, border, hover, focus, and reduced-motion treatment while retaining the existing surfaces, palette, radii, and page layouts; verify changes in the gallery against baseline screenshots.
- [x] 2.2 Polish buttons, inputs, and native selects; add stable label/hint/error associations and loading-button busy semantics. Verify browser tests cover accessible descriptions, disabled/loading non-activation, native required placeholders, and selectable empty filter values.
- [x] 2.3 Polish cards, tables, badges/status pills, and empty states consistently; verify populated/empty gallery examples, unchanged status-to-tone mappings including `AWAITING_PROMOTION`, and no raw palette utilities in changed components.

## 3. Accessible overlays

- [x] 3.1 Replace `NxModal` internals with controlled Reka Dialog primitives, preserving `open`, `close`, and slots; write regression tests before the adapter and verify focus entry/containment, background isolation, Escape/backdrop/close dismissal, opener restoration including fallback, and long-content scrolling.
- [x] 3.2 Replace `NxConfirm` internals with controlled Reka AlertDialog primitives and safe async behavior; write regression tests before the adapter and verify Cancel initial focus, accessible explanation, idle cancellation, outside-click immunity, blocked busy dismissal/duplicate activation, and failure retry.

## 4. Notifications

- [x] 4.1 Integrate one Reka Toast provider/viewport into `NxToasts`, remove queue-owned expiry, and use one local lifetime per rendered toast with Reka expiry disabled; preserve `useToast` callers and verify browser tests for six-second expiry, overlapping hover/focus/window-inactive pause and resume, independent queue removal, polite announcements, and no focus stealing.
- [x] 4.2 Refine notification width, placement, and dismiss affordances; verify keyboard access and dismissal, long-message wrapping, and viewport fit at 390px and increased zoom.

## 5. Gallery and consumer verification

- [x] 5.1 Complete the gallery's applicable state matrix and interactive demonstrations, including busy/failing confirmation, removed dialog opener, long content, and multiple toasts; verify each documented primitive renders and demos perform no service writes, splitting demo components as needed for project file-size guidance.
- [x] 5.2 Review the completed gallery at 390px and 1440px, increased zoom, and reduced motion; verify keyboard/focus behavior and accessible names/descriptions, perform a screen-reader check, and save local before/after evidence with any tooling limitations stated explicitly. (Screen-reader check explicitly waived by the user on 2026-09-08; see verification.md.)
- [x] 5.3 Inspect representative domain-edit/schema-authoring, subscription, and destructive-confirm consumers without submitting live operations; verify component interfaces, layout, navigation, and existing actions remain intact, and record any authentication-blocked browser checks rather than adding bypasses.

## 6. Documentation and final gates

- [x] 6.1 Update `docs/development/design-system.md`, `vue.md`, and `testing.md` for the refined tokens, Nx/Reka boundary, interaction behavior, gallery examples, and browser-test command; verify all documented paths, tokens, and scripts exist.
- [x] 6.2 Update `docs/development/architecture.md` and `decisions.md` with the selective Reka boundary, controlled-overlay and single-timer decisions, and absence of backend side-effect changes; verify these descriptions match the final implementation.
- [x] 6.3 Run `pnpm lint`, `pnpm typecheck`, `pnpm build`, and `pnpm test:ui` from `web/`, plus strict validation of `refine-ui-with-reka`; verify the changed code passes, generated static assets retain the existing script-externalization behavior, and report pre-existing warnings or skipped manual checks faithfully.
- [x] 6.4 Review the final scoped diff and branch status; verify the user-edited logo SVGs remain untouched, no unrelated/backend/auth changes are included, and present the local result for review.
