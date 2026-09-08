## Context

See `proposal.md` for motivation and `specs/web-ui/spec.md` for behavioral contracts.

The frontend is a Nuxt 4/Vue 3 SPA with Tailwind v4 and a small, auto-imported `Nx*` kit. Pages own API calls and open/busy state. `NxModal` currently uses a custom Teleport and Escape listener without explicit focus containment or restoration. `NxConfirm` disables its footer while busy but still inherits modal dismissal. `useToast` owns an unconditional six-second timer, and `NxToasts` renders a polite live region. Simple `NxSelect` instances include legitimate empty-string filter values.

The gallery already demonstrates every primitive and does not declare protected-route middleware. Application startup still loads `config.json`. The web package has lint, typecheck, and static-build scripts but no browser-test runner. The design documentation currently says there is no UI library dependency; that statement must change.

## Goals / Non-Goals

**Goals**

- Isolate Reka within shared components so pages continue using existing props, slots, models, and events.
- Make focus, dismissal, and notification lifetime behavior explicit and testable.
- Review refinements in the gallery before checking their effects on existing consumers.
- Keep deployment static, dependencies bundled locally, and the existing security policy intact.

**Non-Goals**

- No replacement component framework, generated component catalogue, new application state layer, or direct Reka imports in product pages.
- No searchable pickers, sidebar, navigation redesign, light-theme release, external fonts, or icon-package migration.
- No backend changes or repairs to unrelated product-journey gaps. In particular, this pass does not add subscription environment controls.

## Decisions

### 1. Keep the Nx boundary; selectively adopt Reka

Add `reka-ui` as a runtime dependency with explicit component imports inside the wrappers. Use its Dialog, AlertDialog, and Toast primitives; preserve native buttons, inputs, and selects. Keep the existing public interfaces, including `NxModal`'s `open`/`close` and `#actions`, `NxConfirm`'s `open`/`busy`/`confirm`/`cancel`, and `useToast().success/error/info/dismiss`.

This avoids a broad page migration and retains simple-select placeholder and empty-value behavior. Replacing all controls with Reka or adopting a styled framework would add migration work without a corresponding interaction benefit.

### 2. Refine tokens and shared states rather than page structure

Retain the three dark surfaces, blue accent, semantic tones, system font stacks, documented radii, and subtle shadows. Tune contrast, type hierarchy, border treatment, spacing rhythm, and focus/hover states centrally. Polish the owned primitives without changing their data or event contracts. Associate input/select error or hint text through stable IDs and `aria-describedby`; preserve native required/disabled semantics. Buttons keep their labels and expose `aria-busy` while loading.

Use short color/opacity transitions and respect reduced-motion preferences for new motion. Gallery sections can be extracted into small demo components to keep production files within project size guidance. Do not adjust the user-edited brand SVGs.

Ad hoc restyling across all pages was rejected because it would fragment the kit and expand the refresh into a layout redesign.

### 3. Adapt controlled Dialog state and focus deliberately

Map Reka close requests to `NxModal`'s existing `close` event without mutating the caller's state. Use a portal, overlay, content, and title; rely on Reka for background isolation, focus containment, and scroll locking instead of retaining parallel custom listeners.

Pages open dialogs programmatically rather than through a wrapper-owned trigger. Capture the focused opener on the closed-to-open transition and restore it after closing if it is still connected and focusable. Otherwise focus a logical current-page main heading or main container with temporary programmatic focusability, without adding a normal tab stop. Initial focus goes to the first suitable field/control, or content when none exists. Do not reference a nonexistent description when an ordinary modal has only a title and arbitrary slot content.

Bound the panel to the viewport with internal vertical scrolling and narrow-screen gutters. Preserve Escape, backdrop, and close-control dismissal for ordinary modals. A styling-only update was rejected because it would leave the accessibility gap unchanged.

### 4. Give confirmations alert-dialog behavior

Implement `NxConfirm` with AlertDialog rather than an ordinary dismissible modal. Use its title and slotted explanation as the accessible name/description, with initial focus on Cancel. Outside clicks never dismiss; Escape cancels only while idle. Keep the existing caller-controlled async lifecycle: confirming emits once per activation and does not implicitly close the dialog.

While `busy` is true, block close requests and confirmation/cancellation events as well as disabling footer controls. Handle Reka action/cancel default-close behavior explicitly so the parent remains authoritative. Use the same opener-restoration policy as ordinary modals. Share small styling or focus helpers only where the implementations actually need them, rather than adding a general overlay framework.

The intentional behavior change is limited to safer confirmation dismissal. Merely disabling footer buttons was rejected because Escape or outside interaction could still hide an in-flight action.

### 5. Give toast expiry one owner

Place one Toast provider and viewport in the existing single `NxToasts` mount. The composable remains the shared message queue, but remove its unconditional `setTimeout`. Disable Reka's automatic expiry with an infinite duration and give each rendered toast one local six-second lifetime, paused by the union of viewport hover, viewport focus, and window inactivity. Resume the remaining time only after all pause conditions clear, and clean up timers on unmount. Map manual and timed closure to idempotent queue removal. Preserve messages and semantic tones, use polite announcements, and avoid a duplicate outer live region.

The viewport is keyboard-reachable with a documented keyboard shortcut, has labeled dismiss controls, and uses viewport-constrained width and gutters. Notification creation never moves focus. Validate queue behavior for multiple messages and manual/automatic closure. Keeping both timers was rejected because competing timers would defeat pause-on-interaction.

Implementation testing found that Reka 2.10.4 resumes expiry on focus-out even while the viewport remains hovered. The user approved local lifetime ownership on 2026-09-08 to preserve the specified behavior. Reka still owns rendering, announcements, keyboard viewport navigation, and dismiss controls; no package internals are patched.

### 6. Add focused browser behavior tests

Add `@playwright/test` as a development dependency, a web test script, and a focused suite against the gallery. Use a fresh local browser context, intercept `/config.json` with non-secret test values, and block external service traffic. Do not change application authentication or add production test bypasses. Tests exercise real rendered wrappers rather than Reka internals.

Cover focus containment/restoration, all dismissal paths, busy confirmation/retry, toast lifetime/pausing/independent removal, input description links, and select empty values. Use synthetic gallery examples for a removed opener, long content, and a simulated asynchronous failure. Check keyboard behavior and accessible roles/names; supplement automation with visual and screen-reader review rather than claiming full accessibility compliance from DOM checks.

Run `pnpm lint`, `pnpm typecheck`, `pnpm build`, and the new browser-test script. Review at 390px and 1440px widths, increased zoom, and reduced motion. Capture local evidence of the gallery before/after. Inspect representative existing domain-edit/schema-authoring, subscription, and destructive-confirm consumers without submitting real operations; where authentication is unavailable, report the limitation rather than weakening route guards.

Adding broad end-to-end infrastructure or a second DOM unit-test stack was rejected. Browser tests directly address the focus, portal, and timer risks introduced here.

## Risks / Trade-offs

- **Dependency API/version differences** → Check the installed Reka version's public types and component documentation when implementing controlled close, autofocus, and timer handling; behavior tests are the acceptance boundary.
- **Focus restoration without Reka triggers** → Explicit opener capture, a removed-opener gallery case, and fallback tests.
- **Busy action closes automatically** → Controlled state and prevented default dismissal, with tests for Escape, backdrop, Cancel, and repeated confirmation.
- **Timers race or messages linger** → One lifetime owner, idempotent removal, and multiple-toast tests.
- **Shared polish changes many screens** → Gallery-first review, stable contracts, then representative consumer inspection. Do not mix in unrelated fixes.
- **Narrow overlays clash with existing page layout** → Constrain overlays independently and report pre-existing navigation overflow separately; do not redesign navigation.
- **New browser tooling adds installation weight** → Keep it development-only and scope tests to critical shared behavior.
- **Static build or CSP regression** → Keep imports bundled and run the existing generation/externalization build; do not relax headers or add remote assets.

## Migration Plan

1. Work on `feat-ui-reka-refresh`; leave the existing SVG edits untouched.
2. Add the dependency and browser harness, establish baseline evidence, then implement and validate the wrappers and gallery.
3. Update design-system, Vue, testing, architecture, and decision documentation in the same change. Record that no backend write paths or durability changes are introduced.
4. Present local results for review. Push and deployment require separate user authorization.
5. When deployment is authorized, use the existing web static-build/deploy path. No data migration or backend deployment is needed. Roll back by redeploying the previously approved frontend build or reverting only refresh-owned changes, never the user's unrelated edits.
