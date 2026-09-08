## Why

The console already has a recognizable dark visual identity and a shared component kit, but its controls need a more consistent finish and stronger keyboard behavior. Refining that kit with Reka UI primitives can improve dialogs and notifications without replacing the existing layout or rewriting product journeys.

## What Changes

- Refine shared typography, spacing, borders, hover, focus, disabled, and loading states while retaining the dark palette, blue accent, semantic status colors, top navigation, and existing page layouts.
- Use Reka UI beneath the existing modal, confirmation, and toast interfaces for accessible interaction behavior.
- Keep buttons, inputs, native selects, cards, badges, tables, and empty states as Konductor-owned styled components. Restyle simple selects without changing their value or form semantics.
- Expand `/design-system` into the initial review surface for normal, error, disabled, loading, keyboard, and overlay states.
- Preserve page-facing component contracts, API calls, authorization, routing, and static hosting. Searchable pickers, new navigation, and broader page redesign are outside this first pass.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `web-ui`: Refine the documented visual system and gallery; define keyboard-accessible overlays, safe confirmation behavior, and interactable transient notifications.

## Impact

- Frontend: `web/app/components/Nx*.vue`, `web/app/composables/useToast.ts`, `web/app/assets/css/`, and `web/app/pages/design-system.vue`.
- Dependencies: add `reka-ui`, development-only Playwright browser tests, and update the pnpm lockfile; preserve Nuxt SPA/static generation.
- Documentation: update the design system, Vue and testing conventions, architecture description of the component layer, and decision log.
- Verification: web lint/typecheck/build plus browser keyboard, visual, responsive, and accessibility checks against the gallery and representative existing component consumers.
- No backend, AWS resource, authentication, or API contract changes. No deployment or push is included.
- Existing uncommitted logo SVG edits are user work and must remain untouched.
