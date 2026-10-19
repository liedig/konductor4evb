import { expect, test } from '@playwright/test'

import { SCHEMAS, installApi, signIn } from './api'

/**
 * Error, loading, and empty states. Later-registered routes take precedence
 * over the baseline mock, so each test overrides exactly one endpoint.
 */

test.beforeEach(async ({ page }) => {
  await signIn(page, ['platform-admin'])
})

test('environments failure surfaces the message, not a blank page', async ({ page }) => {
  const state = await installApi(page)
  state.failEnvironments = true
  await page.goto('/admin/metadata')

  await expect(page.getByRole('alert').filter({ hasText: 'metadata table unavailable' })).toBeVisible()
  await expect(page.getByText('Protected defaults')).toBeVisible()
})

test('loading state shows progress while the first read is in flight', async ({ page }) => {
  await installApi(page)
  await page.route('**/api/dev/schemas**', async (route) => {
    await new Promise(resolve => setTimeout(resolve, 1200))
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ items: SCHEMAS.slice(0, 2), nextToken: 'page-2' }),
    })
  })
  await page.goto('/domains/orders')

  await expect(page.getByText('Loading…').first()).toBeVisible()
  await expect(page.getByRole('link', { name: 'acme-orders@OrderNotification' })).toBeVisible({ timeout: 10_000 })
})

test('empty environments render guidance instead of a table', async ({ page }) => {
  await installApi(page)
  await page.route('**/api/dev/metadata/environments', async (route) => {
    await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ environments: [] }) })
  })
  await page.goto('/admin/metadata')

  await expect(page.getByText('No environments configured')).toBeVisible()
})

test('an activation audit without history explains itself', async ({ page }) => {
  await installApi(page)
  await page.goto('/admin/metadata')

  await expect(page.getByText('No activations recorded')).toBeVisible()
  await expect(page.getByText('Every committed activation lands here with its actor and transition.')).toBeVisible()
})

test('catalog list failure leaves the panel recoverable', async ({ page }) => {
  await signIn(page, ['platform-admin'])
  await installApi(page)
  await page.route('**/api/dev/schemas**', async (route) => {
    await route.fulfill({
      status: 500,
      contentType: 'application/json',
      body: JSON.stringify({ code: 'InternalError', message: 'catalog index unavailable' }),
    })
  })
  await page.goto('/domains/orders')

  await expect(page.getByText('Schemas could not be loaded — refresh to try again.')).toBeVisible()
})
