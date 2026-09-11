import { expect, test } from '@playwright/test'

import { installApi, signIn } from './api'

/**
 * Role and entitlement boundaries. The UI's guards are conveniences only;
 * each test asserts the surface the API enforces server-side remains the
 * real boundary (read-only presentation for auditors, persona redirects,
 * entitlement-denied catalog detail).
 */

test('auditors get a read-only configuration surface with no editor', async ({ page }) => {
  await signIn(page, ['platform-auditor'])
  await installApi(page)
  await page.goto('/admin/metadata')

  await expect(page.getByText('Read-only', { exact: true })).toBeVisible()
  await expect(page.getByText('Configuration is read-only for auditors.')).toBeVisible()
  await expect(page.getByText('Definition editor')).toBeVisible()
  await expect(page.getByRole('button', { name: 'Save new revision' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Preview draft' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Add field', exact: true })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Prepare activation' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Inspect revision M2' })).toBeVisible()
})

test('team-group sessions cannot open the admin area', async ({ page }) => {
  await signIn(page, ['acme-orders'])
  await installApi(page)
  await page.goto('/admin/metadata')

  await expect(page).toHaveURL(/\/domains$/)
  await expect(page.getByRole('heading', { name: 'My domains' })).toBeVisible()
})

test('unauthenticated navigation lands on the login page', async ({ page }) => {
  await installApi(page)
  await page.goto('/admin/metadata')

  await expect(page).toHaveURL(/\/login/)
})

test('entitlement-denied catalog detail explains the coverage gap', async ({ page }) => {
  await signIn(page, ['acme-orders'])
  await installApi(page)
  await page.route('**/dev/schemas/**', async (route) => {
    await route.fulfill({
      status: 403,
      contentType: 'application/json',
      body: JSON.stringify({ code: 'AccessDenied', message: 'No entitlement covers this schema' }),
    })
  })
  await page.goto('/catalog/orders/acme-orders%40OrderNotification?consumer=acme-consumer')

  await expect(page.getByText('Not covered')).toBeVisible()
  await expect(page.getByText('hold no entitlement covering this schema')).toBeVisible()
})
