import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './tests/ui',
  fullyParallel: true,
  workers: 2,
  retries: 0,
  use: {
    baseURL: 'http://127.0.0.1:3107',
    viewport: { width: 1440, height: 1000 },
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
  },
  webServer: {
    command: 'pnpm exec nuxi dev --host 127.0.0.1 --port 3107',
    url: 'http://127.0.0.1:3107',
    timeout: 120_000,
    reuseExistingServer: false,
  },
})
