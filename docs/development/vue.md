<a id="vue--nuxt-conventions"></a>

# Vue and Nuxt conventions

The web console for Konductor for Amazon EventBridge uses Vue and Nuxt in `web/`. This guide describes the implemented conventions. If the code and this guide differ, update the guide to match the code.

For colors, typography, and design tokens, see [Design system](./design-system.md). The running application renders the component gallery at `/design-system`.

## Application shape

The console uses the following architecture:

- **Single-page application (SPA), no server-side rendering (SSR)**: `ssr: false` in `nuxt.config.ts`. Generate the application with `nuxi generate` and serve it from Amazon Simple Storage Service (Amazon S3) and Amazon CloudFront. Do not add code that depends on SSR behavior.
- **Runtime config over build-time env**: `web/public/config.json` (generated from stack outputs) is loaded once by `plugins/config.client.ts`, validated by `parseKonductorConfig`, and provided app-wide. Read it via `useKonductorConfig()`—never import config values or env vars directly in components.
- **Authentication**: Amazon Cognito through `aws-amplify`, with tokens in `sessionStorage` for each browser tab. `stores/session.ts` is the only Pinia store. It exposes `status`, `username`, `teamGroups`, `isPlatformRole`, `persona`, `homeRoute`, and `accessToken()`.

## API access

Send all API traffic through `useApi()`. It attaches the session bearer token and applies a 30-second timeout. On HTTP 401, it refreshes the token and retries once. A second HTTP 401 redirects to `/login`. It normalizes failures into a typed `ApiError` with `status`, `code`, and `fieldList`.

```vue
<script setup lang="ts">
import { ApiError } from '~/composables/useApi'

const api = useApi()

// ✅ Good
const response = await api.get<DomainDetail>(`/domains/${encodeURIComponent(name)}`)

// ❌ Never - bypasses auth, retry, timeout, and error normalization
const data = await $fetch('/domains')
const { data } = await useFetch('/api/domains')
</script>
```

Bounded contexts get their own thin composable over `useApi()` (`useDomains`, `useCatalog`, `useApprovals`) with one function per endpoint, typed against `~/types/api`. Add new endpoints there, not inline in pages.

### Error handling in pages

Map field-level errors to the form and display other failures in a toast:

```typescript
try {
  await domainsApi.register(body)
  toast.success('Registration request accepted')
} catch (error) {
  if (error instanceof ApiError && error.fieldList.length > 0) {
    // Map server-side field errors onto the form
    for (const field of error.fieldList) {
      if (field.name) fieldErrors[field.name] = field.message ?? 'Invalid'
    }
  } else {
    toast.error(error instanceof ApiError ? error.message : 'Registration request failed')
  }
}
```

Treat HTTP 403 from a scoped endpoint as a user interface (UI) state, not a crash. Render an access-denied or empty state, as in `DomainAccessPanel.vue`.

## Imports

Nuxt auto-imports are **on** and used deliberately:

```vue
<script setup lang="ts">
// ✅ Auto-imported, no import statement: ref, reactive, computed, watch,
//    onMounted, navigateTo, definePageMeta, useId, and everything in
//    composables/ (useApi, useDomains, useCatalog, useApprovals, useToast,
//    useKonductorConfig)
const domainsApi = useDomains()
const items = ref<DomainSummary[]>([])

// ✅ Explicit imports for types, stores, and third-party packages
import { ApiError } from '~/composables/useApi'
import { useSessionStore } from '~/stores/session'
import type { DomainSummary } from '~/types/api'
</script>
```

## Component structure

Always `<script setup lang="ts">`. Group code in this order (see `pages/domains/new.vue` for the canonical form):

1. Type/value imports
2. `definePageMeta` (pages only)
3. Composables and stores (`const session = useSessionStore()`)
4. Constants in `SCREAMING_SNAKE_CASE` (regexes, option lists, limits)
5. Reactive state (`ref`/`reactive`)
6. Computed
7. Plain functions (`validate`, `load`, `submit`)
8. Lifecycle hooks

```vue
<script setup lang="ts">
import type { DirectoryEntry } from '~/types/api'

const api = useApi()
const toast = useToast()

const NAME_PATTERN = /^[a-z0-9][a-z0-9-]{0,62}[a-z0-9]$/

const form = reactive({ domainName: '' })
const busy = ref(false)

const canSubmit = computed(() => NAME_PATTERN.test(form.domainName))

async function submit() { /* ... */ }
</script>
```

## Props, models, emits

Define props, models, and generic components with the following patterns:

```vue
<script setup lang="ts">
// ✅ Type-only props with withDefaults for optionals (NxButton, NxInput)
withDefaults(defineProps<{
  label?: string
  disabled?: boolean
}>(), {
  disabled: false,
})

// ✅ v-model via defineModel (NxInput)
const model = defineModel<string>({ default: '' })

// ✅ Generic components via the generic attribute (NxTable)
// <script setup lang="ts" generic="T extends Record<string, unknown>">
</script>
```

Component-only types can be exported by their owner (`TableColumn` from `NxTable.vue`). Types also used by composables belong in `app/types/` so plain TypeScript checks do not need to resolve Vue files. `BadgeTone` lives in `types/ui.ts`, with a compatibility re-export from `NxBadge.vue`.

## The Nx\* component kit

Build pages from the kit in `app/components/`—`NxButton`, `NxCard`, `NxInput`, `NxSelect`, `NxModal`, `NxConfirm`, `NxBadge`, `NxStatusPill`, `NxTable`, `NxEmptyState`, `NxToasts`—composed with Tailwind utilities and design tokens (`bg-surface`, `text-muted`, `border-border-strong`, `text-danger`, `bg-accent`, ...). **Reka UI is an internal dependency of the overlay and notification wrappers**, not a second page-facing kit. Import its primitives explicitly only inside those wrappers. Buttons, inputs, and simple selects remain native.

```vue
<template>
  <!-- ✅ Good -->
  <NxCard title="Request received">
    <p class="text-sm text-muted">...</p>
    <template #actions>
      <NuxtLink to="/domains"><NxButton variant="secondary">Track in my domains</NxButton></NuxtLink>
    </template>
  </NxCard>

  <!-- ❌ Avoid - raw elements restyled ad hoc when a kit component exists -->
  <button class="rounded bg-blue-600 px-4 py-2 text-white">Submit</button>
</template>
```

- Tables: use `NxTable` with `columns` and `items`, plus `#cell-{key}` and `#empty` slots.
- Variant/tone styling lives in the component as a `Record<Tone, string>` map keyed by the prop (see `NxBadge`, `NxButton`).
- Navigation is `NuxtLink`; actions are `NxButton`. Forms use `@submit.prevent` and a submit-type `NxButton` with `:loading="busy"`.
- Modals use controlled `open` and emit `close`. Confirmations emit `confirm` or `cancel` and never own the API request. Set `busy` during asynchronous confirmation work to prevent dismissal and duplicate actions. Confirmations focus **Cancel** first and do not dismiss when you select the backdrop.

## Forms

Use the form pattern in `pages/domains/new.vue`:

- One `reactive` object for the form, one `reactive<Record<string, string>>` for field errors
- A `validate()` function that populates field errors and returns a boolean; hand-rolled regex checks, no validation library
- A `busy` ref guarding `submit()` (`if (busy.value || !validate()) return`)
- Server `fieldList` errors mapped back onto fields; anything else goes to `toast.error`
- Optional string fields are sent as `undefined` when blank (`form.description || undefined`)

## Feedback

Use the shared toast queue to report asynchronous results:

```typescript
const toast = useToast()
toast.success('Subscription requested')
toast.error(error instanceof ApiError ? error.message : 'Request failed')
```

Toasts are app-wide transient notifications rendered once by `<NxToasts />` in `app.vue`. `useToast` owns only the queue; each `NxToastItem` uses `useToastLifetime` for six seconds of unpaused display time. Hover, focus, or window inactivity pauses expiry until all conditions clear. Do not add a competing queue timeout. Use `NxConfirm`/`NxModal` for destructive or blocking interactions instead of `confirm()`.

## Routing and guards

Apply page middleware according to the required route access:

```vue
<script setup lang="ts">
// Protected page
definePageMeta({ middleware: 'auth' })
// Platform-only page (admin/*)
definePageMeta({ middleware: 'persona' })
</script>
```

Guards control navigation. The API enforces authorization. Use `navigateTo` for programmatic navigation.

## Styling

Apply the shared styling conventions:

- Tailwind utility classes with design-system tokens; **no `<style>` blocks** in components (none exist in the app).
- Conditional classes via `:class` bindings and tone/variant maps, not string interpolation into class names.
- Don't introduce per-component CSS or a CSS framework; extend the design system instead.

## State

Keep state ownership explicit:

- Server data: fetched in pages/components via the composables, held in local refs.
- Cross-component transient UI: module-level shared state inside a composable returning `readonly` refs (`useToast` is the pattern).
- Pinia: only for the session store. Don't add stores for server data.

## Quality gates

The web application has no unit tests. Its quality checks are `pnpm lint`, `pnpm typecheck`, and `pnpm build`. All run in the Web job for continuous integration (CI). Keep these checks passing.
