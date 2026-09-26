import { test as base, expect } from '@playwright/test'

export const test = base.extend({
  page: async ({ page }, use) => {
    await page.route('**/*', async (route) => {
      const url = new URL(route.request().url())
      if (url.origin !== 'http://127.0.0.1:3100' || /^\/dev(?:\/|$)/.test(url.pathname)) {
        await route.abort()
      }
      else if (url.pathname === '/config.json') {
        await route.fulfill({
          json: {
            apiBaseUrl: '/dev',
            region: 'ap-southeast-2',
            userPoolId: 'ap-southeast-2_GalleryTest',
            userPoolClientId: 'gallerytestclient',
            environments: ['dev', 'staging', 'prod'],
          },
        })
      }
      else {
        await route.continue()
      }
    })
    await use(page)
  },
})

export { expect }
