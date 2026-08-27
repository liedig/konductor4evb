# Vue & Nuxt Conventions

Conventions for the Konductor web console in `web/`. Verified against the codebase; when this doc and the code disagree, the code wins — fix the doc.

For colors, typography, and design tokens, see [design-system.md](./design-system.md) (rendered live at `/design-system` in the running app).

## Application Shape

- **SPA, no SSR**: `ssr: false` in `nuxt.config.ts`; statically generated (`nuxi generate`) and served from S3 + CloudFront. There is no server side — never write code that depends on SSR behavior, and don't add `useFetch` "for SSR".
- **Runtime config over build-time env**: `web/public/config.json` (generated from stack outputs) is loaded once by `plugins/config.client.ts`, validated by `parseKonductorConfig`, and provided app-wide. Read it via `useKonductorConfig()` — never import config values or env vars directly in components.
- **Auth**: Cognito via aws-amplify, tokens in `sessionStorage` (per browser tab). `stores/session.ts` is the **only** Pinia store; it exposes `status`, `username`, `teamGroups`, `isPlatformRole`, `persona`, `homeRoute`, and `accessToken()`.

## API Access

**All** API traffic goes through `useApi()`. It attaches the session Bearer token, refreshes once and retries on 401, redirects to `/login` on a second 401, applies a 30s timeout, and normalizes failures into a typed `ApiError` (`status`, `code`, `fieldList`).

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

### Error Handling in Pages

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

A 403 from a scoped endpoint is a UI state, not a crash — render a forbidden/empty state (see `DomainAccessPanel.vue`).

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

## Component Structure

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

## Props, Models, Emits

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

Small shared types are exported from the component that owns them (`BadgeTone` from `NxBadge.vue`) and imported as types elsewhere.

## The Nx\* Component Kit

There is **no UI component library dependency**. Build pages from the kit in `app/components/` — `NxButton`, `NxCard`, `NxInput`, `NxSelect`, `NxModal`, `NxConfirm`, `NxBadge`, `NxStatusPill`, `NxTable`, `NxEmptyState`, `NxToasts` — composed with Tailwind utilities and design tokens (`bg-surface`, `text-muted`, `border-border-strong`, `text-danger`, `bg-accent`, ...).

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

- Tables: `NxTable` with `columns`/`items` and `#cell-{key}` / `#empty` slots.
- Variant/tone styling lives in the component as a `Record<Tone, string>` map keyed by the prop (see `NxBadge`, `NxButton`).
- Navigation is `NuxtLink`; actions are `NxButton`. Forms use `@submit.prevent` and a submit-type `NxButton` with `:loading="busy"`.

## Forms

The established form pattern (see `pages/domains/new.vue`):

- One `reactive` object for the form, one `reactive<Record<string, string>>` for field errors
- A `validate()` function that populates field errors and returns a boolean; hand-rolled regex checks, no validation library
- A `busy` ref guarding `submit()` (`if (busy.value || !validate()) return`)
- Server `fieldList` errors mapped back onto fields; anything else goes to `toast.error`
- Optional string fields are sent as `undefined` when blank (`form.description || undefined`)

## Feedback

```typescript
const toast = useToast()
toast.success('Subscription requested')
toast.error(error instanceof ApiError ? error.message : 'Request failed')
```

Toasts are app-wide transient notifications rendered once by `<NxToasts />` in `app.vue`. Use `NxConfirm`/`NxModal` for destructive or blocking interactions instead of `confirm()`.

## Routing and Guards

```vue
<script setup lang="ts">
// Protected page
definePageMeta({ middleware: 'auth' })
// Platform-only page (admin/*)
definePageMeta({ middleware: 'persona' })
</script>
```

The guards are navigation hygiene only — the API is the enforcing boundary. Programmatic navigation uses `navigateTo`.

## Styling

- Tailwind utility classes with design-system tokens; **no `<style>` blocks** in components (none exist in the app).
- Conditional classes via `:class` bindings and tone/variant maps, not string interpolation into class names.
- Don't introduce per-component CSS or a CSS framework; extend the design system instead.

## State

- Server data: fetched in pages/components via the composables, held in local refs.
- Cross-component transient UI: module-level shared state inside a composable returning `readonly` refs (`useToast` is the pattern).
- Pinia: only for the session store. Don't add stores for server data.

## Quality Gates

No unit tests exist for the web app today; the gates are `pnpm lint`, `pnpm typecheck`, and `pnpm build` (all run in CI's Web job). Keep them green.
