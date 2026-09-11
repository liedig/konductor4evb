# Design System

The Konductor console has one visual identity, defined once as code and documented here. Every token and primitive listed below maps to a real code path; the living gallery at `/design-system` (excluded from navigation) renders each primitive against the current build. Reka UI supplies unstyled dialog, confirmation, and notification behavior behind the existing `Nx*` interfaces. Konductor owns the styling and native form controls. When you change a token or primitive, update this document in the same change.

## Principles

**Clean, airy, professional.** Konductor balances enterprise credibility with approachable clarity. The interface avoids visual noise while maintaining enough density for serious workflows.

- **Layered depth** — three surface levels only: `background` (page) → `surface` (cards) → `surface-raised` (interactive elements, popovers, toasts). Never invent a fourth layer.
- **Generous whitespace** — pages breathe (`px-6 py-10` shells, `gap-10` between sections, cards pad `p-6`). Whitespace is the primary grouping device; dividers are rare.
- **Color with intent** — status colors carry meaning and are never decorative: green = ready (`success`), blue = active/in-progress (`info`), red = urgent (`danger`), amber = pending (`warning`), grey = inactive (`neutral`).
- **Consistent radius** — `rounded-lg` for controls and pills, `rounded-xl` for cards, modals, and empty states. No other radii.
- **Subtle shadows** — `shadow-sm` is the only permitted elevation, applied to cards, modals, and toasts. Never heavier.

**Dark-from-day-one.** Dark is the default theme, not an inversion pass. Tokens are declared once in `@theme` (which emits them at `:root`); a light palette exists under `[data-theme="light"]` but is unpolished and not yet shipped.

## Tokens

Canonical source: `web/app/assets/css/tokens.css` (Tailwind v4 `@theme`). Components consume semantic utilities (`bg-surface`, `text-muted`, `border-border`…), never raw palette colors.

| Token | Utility prefix | Role |
|---|---|---|
| `--color-background` | `bg-background` | Page background (layer 0) |
| `--color-surface` | `bg-surface` | Cards, panels (layer 1) |
| `--color-surface-raised` | `bg-surface-raised` | Interactive elements, toasts, modal backdrop content (layer 2) |
| `--color-foreground` | `text-foreground` | Primary text |
| `--color-muted` | `text-muted` | Secondary text, labels |
| `--color-faint` | `text-faint` | Placeholders, disabled, icons |
| `--color-border` | `border-border` | Default card/table borders |
| `--color-border-strong` | `border-border-strong` | Input borders, dashed empty-state borders |
| `--color-accent` / `--color-accent-strong` | `bg-accent`, `bg-accent-strong` | Primary action fill and hover |
| `--color-on-accent` | `text-on-accent` | Text on accent fills |
| `--color-success` | `text-success`, `bg-success/15` | Ready states |
| `--color-info` | `text-info`, `bg-info/15` | Active/in-progress states |
| `--color-warning` | `text-warning`, `bg-warning/15` | Pending states |
| `--color-danger` | `text-danger`, `bg-danger/15` | Urgent/error states |
| `--color-neutral` | `text-neutral`, `bg-neutral/15` | Inactive/removed states |
| `--color-focus` | focus outline | `:focus-visible` outline color |
| `--font-sans` | `font-sans` | System sans stack (default body font) |
| `--font-mono` | `font-mono` | Code, ARNs, JSON payloads |

Spacing uses the default Tailwind scale; the rhythm conventions are `gap-1.5` (label→control), `gap-4` (form fields), `p-6` (cards), `gap-10` (page sections).

Body text uses a 1.6 line height; headings use slightly tightened tracking. Form controls have strong borders on the background surface, while table headings and secondary buttons use the raised surface. Keyboard focus has a two-pixel semantic outline. Disabled controls do not receive hover styling; loading buttons retain their label and expose `aria-busy`. Reduced-motion preferences suppress animation and transitions.

## Primitives

All primitives live in `web/app/components/` and are auto-imported (`<NxButton />` …).

| Primitive | Path | Notes |
|---|---|---|
| Button | `NxButton.vue` | Variants `primary`/`secondary`/`danger`/`ghost`, sizes `sm`/`md`, `loading` spinner |
| Input | `NxInput.vue` | `v-model`, associated label/hint/error, required and disabled semantics |
| Select | `NxSelect.vue` | Native select; `v-model`, `options`, associated label/hint/error; preserves empty filter values |
| Badge | `NxBadge.vue` | Tones `success`/`info`/`warning`/`danger`/`neutral` |
| Status pill | `NxStatusPill.vue` | The single mapping from lifecycle status strings (`ACTIVE`, `PENDING_APPROVAL`…) to tones — reuse it, don't re-map |
| Card | `NxCard.vue` | Optional `title` + `#actions` header slot |
| Table | `NxTable.vue` | `columns` + `items`; per-column `cell-<key>` slots, `#empty` slot |
| Modal | `NxModal.vue` | Reka Dialog, controlled `open`/`close`, Escape/backdrop close, `#actions` footer slot |
| Confirm | `NxConfirm.vue` | Reka AlertDialog, controlled `open`/`busy`/`confirm`/`cancel`; `danger` styling |
| Toast | `NxToasts.vue` + `NxToastItem.vue` + `composables/useToast.ts` | `useToast().success/error/info(message)`; one provider and viewport in `app.vue` |
| Empty state | `NxEmptyState.vue` | `title`, `body`, optional `#icon` and `#actions` |

## Interaction contracts

- **Modal**: focus enters the first suitable form field (or an available control), remains inside, and returns to the opener on close. If the opener disappears, `composables/useDialogFocus.ts` focuses the current main heading or main container. Background content is isolated and scroll-locked. Panels use `.nx-overlay`/`.nx-dialog` in `assets/css/main.css`, viewport gutters, and internal scrolling.
- **Confirmation**: the explanatory slot supplies the accessible description. Cancel receives initial focus. Idle Escape cancels; outside clicks do not. While busy, confirmation, cancellation, and Escape dismissal are blocked. Pages own async work and decide when to close. Ordinary modals retain their existing dismissal behavior and do not gain a busy prop.
- **Notification**: polite announcements do not move focus. Press **F8** to reach the viewport, then Tab to navigate notifications and dismiss controls. Each message expires after six seconds of unpaused display time. Hover, keyboard focus, and window inactivity pause expiry; it resumes only when all conditions clear. `composables/useToastLifetime.ts` owns the single timer per rendered item, with Reka expiry disabled. This avoids Reka 2.10.4's focus-out/hover overlap bug. Removal cleans up the timer.
- **Form control**: labels and displayed hints/errors have stable ID associations. Inputs/selects expose `aria-invalid`; errors take precedence over hints. Selects remain native, including empty-valued “All…” filters and required placeholders.

The gallery demonstrates error/disabled/loading states, awaiting-promotion status, long dialogs, a removed opener, simulated confirmation failure/retry, and multiple notifications. Demo components live in `components/gallery/` and never perform service writes. See [testing.md](./testing.md#browser-ui-tests-playwright) for browser checks and manual accessibility review.

## Page patterns

- **Form** (e.g. domain request): `NxCard` wrapping a single-column `grid gap-4` of labeled inputs; submit row right-aligned with primary button; server-side field errors (`ApiError.fieldList`) map onto the matching input's `error` prop.
- **List + filter**: page header (`text-2xl font-semibold` title, muted subtitle, primary action on the right), filter row of `NxSelect`/`NxInput`, then `NxTable` with `NxStatusPill` cells; pagination via a "Load more" ghost button when `nextToken` is present.
- **Detail**: header with name + status pill + actions; stacked `NxCard` sections (overview, related resources); read-only fields render as definition lists, never disabled inputs.
- **Inbox**: list-first page where each row is one actionable item with age, primary approve action, and destructive reject behind a justification form; empty inbox gets an explicit `NxEmptyState`.

Status-to-tone mapping is centralized in `NxStatusPill`; subscribe/enable async flows settle on success/failure toasts and the polling helper rather than bespoke spinners.
