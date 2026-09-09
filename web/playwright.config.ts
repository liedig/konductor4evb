import { defineConfig } from '@playwright/test'

/**
 * Isolated-browser UI tests (`pnpm test:ui`).
 *
 * The suite runs against the dev server with no external API: every
 * `/dev/**` call is fulfilled by the in-test route mock in
 * `tests/ui/api.ts`, and identity comes from the dev-only
 * `konductorTestSession` sessionStorage seam (stores/session.ts) — the
 * same seam the manual browser verification used. No real credentials,
 * no deployed stack, no side effects.
 *
 * One worker: the mock is per-test state and the dev server is shared.
 */
export default defineConfig({
  testDir: './tests/ui',
  timeout: 30_000,
  fullyParallel: false,
  workers: 1,
  retries: 0,
  reporter: [['list']],
  use: {
    baseURL: 'http://127.0.0.1:3100',
    trace: 'retain-on-failure',
  },
  webServer: {
    command: 'pnpm exec nuxi dev --port 3100 --host 127.0.0.1',
    url: 'http://127.0.0.1:3100/config.json',
    reuseExistingServer: false,
    timeout: 180_000,
  },
})
