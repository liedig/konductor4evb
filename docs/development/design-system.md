# Design system

The console for Konductor for Amazon EventBridge uses a shared design system. Every token and primitive in this guide maps to an implementation. The gallery at `/design-system`, which navigation excludes, renders each primitive against the current build.

Reka UI supplies unstyled dialog, confirmation, and notification behavior behind the existing `Nx*` interfaces. Konductor owns the styling and native form controls. When you change a token or primitive, update this document in the same change.

## Principles

Use the following principles to keep pages consistent and readable:

- **Layered depth** – Use only three surface levels: `background` for pages, `surface` for cards, and `surface-raised` for interactive elements, popovers, and toasts.
- **Whitespace** – Use `px-6 py-10` for page shells, `gap-10` between sections, and `p-6` for card padding. Group content primarily with whitespace.
- **Status colors** – Use green for ready (`success`), blue for active or in progress (`info`), red for urgent (`danger`), amber for pending (`warning`), and gray for inactive (`neutral`). Identify status by its label, not by color alone.
- **Consistent radius** – Use `rounded-lg` for controls and pills, and `rounded-xl` for cards, modals, and empty states. Do not add other radii.
- **Subtle shadows** – Use only `shadow-sm` for cards, modals, and toasts.

Dark is the default theme. Declare tokens once in `@theme`, which emits them at `:root`. The light palette under `[data-theme="light"]` is unfinished and has not shipped.

## Tokens

The canonical token definitions are in `web/app/assets/css/tokens.css`, using Tailwind v4 `@theme`. Components use semantic utilities such as `bg-surface`, `text-muted`, and `border-border`, not raw palette colors. The documentation site mirrors the color values in `docs/stylesheets/brand.css`; when you change a color token, update that file in the same change. The following table describes each token.

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
| `--color-accent`, `--color-accent-strong` | `bg-accent`, `bg-accent-strong` | Primary action fill and hover |
| `--color-on-accent` | `text-on-accent` | Text on accent fills |
| `--color-success` | `text-success`, `bg-success/15` | Ready states |
| `--color-info` | `text-info`, `bg-info/15` | Active/in-progress states |
| `--color-warning` | `text-warning`, `bg-warning/15` | Pending states |
| `--color-danger` | `text-danger`, `bg-danger/15` | Urgent/error states |
| `--color-neutral` | `text-neutral`, `bg-neutral/15` | Inactive/removed states |
| `--color-focus` | focus outline | `:focus-visible` outline color |
| `--font-sans` | `font-sans` | System sans stack (default body font) |
| `--font-mono` | `font-mono` | Code, Amazon Resource Names (ARNs), JSON payloads |

Spacing uses the default Tailwind scale; the rhythm conventions are `gap-1.5` (label→control), `gap-4` (form fields), `p-6` (cards), `gap-10` (page sections).

Body text uses a 1.6 line height; headings use slightly tightened tracking. Form controls have strong borders on the background surface, while table headings and secondary buttons use the raised surface. Keyboard focus has a two-pixel semantic outline. Disabled controls do not receive hover styling; loading buttons retain their label and expose `aria-busy`. Reduced-motion preferences suppress animation and transitions.

## Primitives

All primitives are in `web/app/components/`. Nuxt imports them automatically, including `<NxButton />`. The following table lists the supported primitives.

| Primitive | Path | Notes |
|---|---|---|
| Button | `NxButton.vue` | Variants `primary`/`secondary`/`danger`/`ghost`, sizes `sm`/`md`, `loading` spinner |
| Input | `NxInput.vue` | `v-model`, associated label/hint/error, required and disabled semantics |
| Select | `NxSelect.vue` | Native select; `v-model`, `options`, associated label/hint/error; preserves empty filter values |
| Badge | `NxBadge.vue` | Tones `success`/`info`/`warning`/`danger`/`neutral` |
| Status pill | `NxStatusPill.vue` | Maps lifecycle status strings, including `ACTIVE`, `PENDING_APPROVAL`, and the promotion phases (`VALIDATING`, `REPLAYING`, `REPINNING`, `COMPLETED`, `FAILED`, `CANCELLED`), to tones. Reuse this mapping. |
| Card | `NxCard.vue` | Optional `title` + `#actions` header slot |
| Table | `NxTable.vue` | `columns` + `items`; per-column `cell-<key>` slots, `#empty` slot |
| Modal | `NxModal.vue` | Reka Dialog, controlled `open`/`close`, Escape/backdrop close, `#actions` footer slot |
| Confirm | `NxConfirm.vue` | Reka AlertDialog, controlled `open`/`busy`/`confirm`/`cancel`; `danger` styling |
| Toast | `NxToasts.vue` + `NxToastItem.vue` + `composables/useToast.ts` | `useToast().success/error/info(message)`; one provider and viewport in `app.vue` |
| Empty state | `NxEmptyState.vue` | `title`, `body`, optional `#icon` and `#actions` |

## Interaction contracts

Preserve the following behavior whenever you change an overlay, notification, or form control:

- **Modal**: focus enters the first suitable form field (or an available control), remains inside, and returns to the opener on close. If the opener disappears, `composables/useDialogFocus.ts` focuses the current main heading or main container. Background content is isolated and scroll-locked. Panels use `.nx-overlay`/`.nx-dialog` in `web/app/assets/css/main.css`, viewport gutters, and internal scrolling.
- **Confirmation**: the explanatory slot supplies the accessible description. **Cancel** receives initial focus. Escape cancels an idle confirmation. Selecting outside it does not. While busy, the confirmation blocks confirmation, cancellation, and Escape dismissal. Pages own asynchronous work and decide when to close. Ordinary modals retain their existing dismissal behavior and do not gain a busy prop.
- **Notification**: polite announcements do not move focus. Press **F8** to reach the viewport, then Tab to navigate notifications and dismiss controls. Each message expires after six seconds of unpaused display time. Hover, keyboard focus, and window inactivity pause expiry; it resumes only when all conditions clear. `composables/useToastLifetime.ts` owns the single timer per rendered item, with Reka expiry disabled. This avoids Reka 2.10.4's focus-out/hover overlap bug. Removal cleans up the timer.
- **Form control**: labels, displayed hints, and errors have stable ID associations. Inputs and selects expose `aria-invalid`. Errors take precedence over hints. Selects remain native, including empty-valued "All…" filters and required placeholders.

The gallery demonstrates error, disabled, and loading states, awaiting-promotion status, long dialogs, a removed opener, confirmation failures and retries, and multiple notifications. Demo components are in `web/app/components/gallery/` and never perform service writes. For browser checks and manual accessibility review, see [Browser UI tests (Playwright)](./testing.md#browser-ui-tests-playwright).

## Page patterns

Use these patterns when composing pages:

- **Form**, for example a domain request: use `NxCard` around a single-column `grid gap-4` of labeled inputs. Right-align the submit row with a primary button. Map server-side field errors (`ApiError.fieldList`) to the matching input's `error` prop.
- **List + filter**: page header (`text-2xl font-semibold` title, muted subtitle, primary action on the right), filter row of `NxSelect`/`NxInput`, then `NxTable` with `NxStatusPill` cells; pagination via a "Load more" ghost button when `nextToken` is present.
- **Detail**: header with name + status pill + actions; stacked `NxCard` sections (overview, related resources); read-only fields render as definition lists, never disabled inputs.
- **Inbox**: list-first page where each row is one actionable item with age, primary approve action, and destructive reject behind a justification form; empty inbox gets an explicit `NxEmptyState`.
- **Pipeline**: an ordered strip of environment nodes (configured order, never alphabetical) showing per-node state as `NxBadge`s and a per-target action; long-running transitions poll a status endpoint and report phases with `NxStatusPill`, settling on a toast. Used by the producer schema page's promotion pipeline. Clickable badges that open an inspection overlay are a documented raw-control exception: a `button` wrapping the `NxBadge` (no button primitive renders as a badge), with a descriptive `aria-label` and the standard `:focus-visible` outline. Used by the version history's per-environment content viewer.

Status-to-tone mapping is centralized in `NxStatusPill`; subscribe/enable async flows settle on success/failure toasts and the polling helper rather than bespoke spinners.
