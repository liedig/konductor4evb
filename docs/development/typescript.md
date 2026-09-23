# TypeScript conventions

The web console for Konductor for Amazon EventBridge uses TypeScript in `web/`. The backend uses Rust. This guide describes the implemented conventions. If the code and this guide differ, update the guide to match the code.

For component structure, the API layer, and the Nx\* kit, see [Vue and Nuxt conventions](./vue.md).

## Toolchain

The continuous integration (CI) Web job uses the following toolchain:

- Node.js 24 and pnpm (version pinned by the `packageManager` field in `web/package.json`)
- `nuxt.config.ts` + `tsconfig.json` extending Nuxt's generated `.nuxt/tsconfig.json` (strict by default)
- Quality checks: `pnpm lint` (ESLint with `@nuxt/eslint` defaults), `pnpm typecheck` (`nuxi typecheck`, vue-tsc), and `pnpm build`

## Formatting

Follow the formatting rules enforced by the Nuxt ESLint stylistic preset:

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

Use the following import conventions:

- `import type` for type-only imports (`import type { DomainSummary } from '~/types/api'`)
- Explicit value imports for stores and third-party packages. Nuxt automatically imports Vue primitives and `composables/`. See [Imports](./vue.md#imports).
- Path alias `~/` for everything under `app/`, never deep relative chains

## API types

`app/types/api.ts` mirrors the OpenAPI specification in `api/openapi-oauth.yaml`. These types are handwritten. When the API changes, update both files in the same change.

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

## Runtime validation at boundaries

The console does not use a validation library. Parse `unknown` into typed values with explicit guards at two boundaries: startup configuration and API error bodies. Use `parseKonductorConfig` in `types/config.ts` for configuration and `isErrorBody` in `useApi.ts` for errors.

```typescript
function isErrorBody(data: unknown): data is ApiErrorBody {
  return (
    typeof data === 'object'
    && data !== null
    && typeof (data as ApiErrorBody).code === 'string'
  )
}
```

Accept external data as `unknown`, then narrow its type. Do not use `any`.

## Error handling

Handle errors with the following conventions:

- The API layer throws typed `ApiError` objects. Narrow them with `instanceof`. See [Error handling in pages](./vue.md#error-handling-in-pages).
- Normalize unknown caught values before surfacing them: `error instanceof Error ? error.message : 'Request failed'`
- Handle caught errors with typed error handling or a toast. Do not use only `console.error`.

## Everyday idioms

Use the following patterns for defaults, optional values, and constrained types:

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
- Requests must not hang forever—`useApi` already applies `AbortSignal.timeout(30_000)`; keep that pattern for any new network path
- Prefer derived, non-mutating transforms (`map`/`filter`/`toSorted`) over in-place mutation of shared state; form arrays are the sanctioned exception (v-model binds into a `reactive` form object)

## State and reactivity types

Use explicit types where inference does not provide the required constraints:

- `ref`/`reactive` with explicit type parameters when inference is too loose: `ref<Entitlement[]>([])`, `reactive<Record<string, string>>({})`
- Shared state from composables is exposed `readonly` with setter functions (`useToast`)
- The single Pinia store (`stores/session.ts`) is setup-style with computed derivations, mirroring the backend principal contract—extend it rather than adding parallel session state
