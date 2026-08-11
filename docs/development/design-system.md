# Design System

The Nexus console has one visual identity, defined once as code and documented here. Every token and primitive listed below maps to a real code path; the living gallery at `/design-system` (excluded from navigation) renders each primitive against the current build. When you change a token or primitive, update this document in the same change.

## Principles

**Clean, airy, professional.** Nexus balances enterprise credibility with approachable clarity. The interface avoids visual noise while maintaining enough density for serious workflows.

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

## Primitives

All primitives live in `web/app/components/` and are auto-imported (`<NxButton />` …).

| Primitive | Path | Notes |
|---|---|---|
| Button | `NxButton.vue` | Variants `primary`/`secondary`/`danger`/`ghost`, sizes `sm`/`md`, `loading` spinner |
| Input | `NxInput.vue` | `v-model`, `label`/`hint`/`error`, required marker |
| Select | `NxSelect.vue` | `v-model`, `options` prop, same label/hint/error contract |
| Badge | `NxBadge.vue` | Tones `success`/`info`/`warning`/`danger`/`neutral` |
| Status pill | `NxStatusPill.vue` | The single mapping from lifecycle status strings (`ACTIVE`, `PENDING_APPROVAL`…) to tones — reuse it, don't re-map |
| Card | `NxCard.vue` | Optional `title` + `#actions` header slot |
| Table | `NxTable.vue` | `columns` + `items`; per-column `cell-<key>` slots, `#empty` slot |
| Modal | `NxModal.vue` | Teleported, Escape/backdrop close, `#actions` footer slot |
| Confirm | `NxConfirm.vue` | Modal specialized for destructive/irreversible confirms (`danger` prop) |
| Toast | `NxToasts.vue` + `composables/useToast.ts` | `useToast().success/error/info(message)`; rendered once in `app.vue` |
| Empty state | `NxEmptyState.vue` | `title`, `body`, optional `#icon` and `#actions` |

## Page patterns

- **Form** (e.g. domain request): `NxCard` wrapping a single-column `grid gap-4` of labeled inputs; submit row right-aligned with primary button; server-side field errors (`ApiError.fieldList`) map onto the matching input's `error` prop.
- **List + filter**: page header (`text-2xl font-semibold` title, muted subtitle, primary action on the right), filter row of `NxSelect`/`NxInput`, then `NxTable` with `NxStatusPill` cells; pagination via a "Load more" ghost button when `nextToken` is present.
- **Detail**: header with name + status pill + actions; stacked `NxCard` sections (overview, related resources); read-only fields render as definition lists, never disabled inputs.
- **Inbox**: list-first page where each row is one actionable item with age, primary approve action, and destructive reject behind a justification form; empty inbox gets an explicit `NxEmptyState`.

Status-to-tone mapping is centralized in `NxStatusPill`; subscribe/enable async flows settle on success/failure toasts and the polling helper rather than bespoke spinners.
