# TypeScript Conventions

TypeScript conventions for the Konductor web console in `web/` — the only TypeScript in the repo (the backend is Rust). Verified against the codebase; when this doc and the code disagree, the code wins — fix the doc.

Vue/Nuxt-specific patterns (component structure, the API layer, the Nx\* kit) live in [vue.md](./vue.md).

## Toolchain

- Node 24 (CI), pnpm (version pinned by the `packageManager` field in `web/package.json`)
- `nuxt.config.ts` + `tsconfig.json` extending Nuxt's generated `.nuxt/tsconfig.json` (strict by default)
- Gates, all run in CI's Web job: `pnpm lint` (ESLint via `@nuxt/eslint` defaults), `pnpm typecheck` (`nuxi typecheck`, vue-tsc), `pnpm build`

## Formatting

Enforced by the Nuxt ESLint stylistic preset — don't fight it by hand:

- Single quotes, **no semicolons**, trailing commas in multiline literals
- 2-space indent

```typescript
// ✅ Matches the codebase
const tones: Record<BadgeTone, string> = {
  success: 'bg-success/15 text-success',
  neutral: 'bg-neutral/15 text-neutral',
}
```

## Imports

- `import type` for type-only imports (`import type { DomainSummary } from '~/types/api'`)
- Value imports explicit for stores and third-party packages; Vue primitives and `composables/` are Nuxt auto-imports (see vue.md)
- Path alias `~/` for everything under `app/`, never deep relative chains

## API Types

`app/types/api.ts` hand-mirrors the OpenAPI spec (`api/openapi-oauth.yaml`) — hand-written by design; when the API changes, update both in the same change.

- String-literal unions for enums (`DomainStatus`, `BadgeTone`) instead of stringly-typed values
- Optional fields use `?` (they are absent, not `null`)

```typescript
export type DomainStatus =
  | 'PENDING_APPROVAL'
  | 'PROVISIONING'
  | 'ACTIVE'
  | 'INACTIVE'
// ...

export interface DomainSummary {
  domainName: string
  status?: DomainStatus
  environments?: string[]
}
```

## Runtime Validation at Boundaries

No validation library. Parse `unknown` into typed values with hand-rolled guards at exactly two boundaries: boot-time config (`parseKonductorConfig` in `types/config.ts`) and API error bodies (`isErrorBody` in `useApi.ts`).

```typescript
function isErrorBody(data: unknown): data is ApiErrorBody {
  return (
    typeof data === 'object'
    && data !== null
    && typeof (data as ApiErrorBody).code === 'string'
  )
}
```

`any` is not used; external/unknown data enters as `unknown` and is narrowed.

## Error Handling

- The API layer throws typed `ApiError` (see vue.md); callers narrow with `instanceof`
- Normalize unknown caught values before surfacing them: `error instanceof Error ? error.message : 'Request failed'`
- Never swallow: catch → typed handling or a toast, not a bare `console.error`

## Everyday Idioms

```typescript
// ✅ ?? for defaults — preserves 0, false, ''
entitlements.value = response.entitlements ?? []

// ❌ || drops legitimate falsy values
const count = response.count || 0

// ✅ Optional chaining over nested null checks
const slug = details?.slug ?? condition.questionId

// ✅ Constrained generics over bare T
<script setup lang="ts" generic="T extends Record<string, unknown>">

// ✅ Blank optional form fields become undefined at the API edge
description: form.description || undefined
```

- `undefined` means "absent"; don't introduce `null` except where the API contract uses it
- async/await everywhere; no raw `.then()` chains
- Requests must not hang forever — `useApi` already applies `AbortSignal.timeout(30_000)`; keep that pattern for any new network path
- Prefer derived, non-mutating transforms (`map`/`filter`/`toSorted`) over in-place mutation of shared state; form arrays are the sanctioned exception (v-model binds into a `reactive` form object)

## State and Reactivity Types

- `ref`/`reactive` with explicit type parameters when inference is too loose: `ref<Entitlement[]>([])`, `reactive<Record<string, string>>({})`
- Shared state from composables is exposed `readonly` with setter functions (`useToast`)
- The single Pinia store (`stores/session.ts`) is setup-style with computed derivations, mirroring the backend principal contract — extend it rather than adding parallel session state
