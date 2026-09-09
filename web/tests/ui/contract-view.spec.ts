import { expect, test, type Page } from '@playwright/test'

import { installApi, signIn } from './api'

/**
 * Composed-contract inspection (spec 6.5): independent business/metadata
 * version badges, exact historical pairs (never a current-metadata
 * substitution), and publisher previews that stay business-only.
 */

function card(page: Page, title: string) {
  return page.locator('section').filter({
    has: page.getByRole('heading', { name: title, exact: true }),
  })
}

test.beforeEach(async ({ page }) => {
  await signIn(page, ['platform-admin'])
})

test('catalog detail shows independent version and revision badges per environment', async ({ page }) => {
  await installApi(page)
  await page.goto('/catalog/orders/acme-orders%40OrderNotification')

  const badges = card(page, 'Latest version per environment')
  await expect(badges.getByText('prod v3', { exact: true })).toBeVisible()
  await expect(badges.getByText('dev v2', { exact: true })).toBeVisible()
  await expect(badges.getByText('dev M2', { exact: true })).toBeVisible()
  await expect(badges.getByText('prod M0', { exact: true })).toHaveCount(0)
})

test('contract view auto-loads the current pair and exposes all three documents', async ({ page }) => {
  await installApi(page)
  await page.goto('/catalog/orders/acme-orders%40OrderNotification')

  const contract = card(page, 'Composed contract')
  await expect(contract.getByText('dev V2', { exact: true })).toBeVisible()
  await expect(contract.getByText('dev M2', { exact: true })).toBeVisible()
  await expect(contract.getByText(/digest d{12}/)).toBeVisible()

  await contract.getByRole('button', { name: 'Inherited metadata' }).click()
  await expect(contract.getByText('read-only for every caller')).toBeVisible()
  await contract.getByRole('button', { name: 'Raw business' }).click()
  await expect(contract.getByText('The raw business document, unchanged by metadata activation')).toBeVisible()
  await expect(contract.getByText('"metadata"')).toHaveCount(0)
})

test('historical inspection demands both selectors and never substitutes current metadata', async ({ page }) => {
  await installApi(page)
  await page.goto('/catalog/orders/acme-orders%40OrderNotification')

  const contract = card(page, 'Composed contract')
  await page.getByLabel('Historical pair').check()
  await page.getByLabel('Business version').fill('2')
  await contract.getByRole('button', { name: 'Load contract' }).click()
  await expect(contract.getByText('Historical inspection needs both the business version and the metadata revision (0 is the legacy selector).')).toBeVisible()

  await page.getByLabel('Business version').fill('')
  await contract.getByRole('button', { name: 'Load contract' }).click()
  await expect(contract.getByText('Enter the business version and metadata revision, or switch back to the current pair.')).toBeVisible()

  await page.getByLabel('Business version').fill('2')
  await page.getByLabel('Metadata revision').fill('0')
  await contract.getByRole('button', { name: 'Load contract' }).click()
  await expect(contract.getByText('dev V2', { exact: true })).toBeVisible()
  await expect(contract.getByText('dev M0', { exact: true })).toBeVisible()
  await expect(contract.getByText('Historical — explicit selection')).toBeVisible()

  await page.getByLabel('Business version').fill('9')
  await contract.getByRole('button', { name: 'Load contract' }).click()
  await expect(contract.getByText('No historical contract for v9 M0 in dev')).toBeVisible()
})

test('authoring preview composes unsaved business content without envelope fields', async ({ page }) => {
  await installApi(page)
  await page.goto('/domains/orders')

  await page.getByRole('button', { name: 'New version' }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByText('Target environment: dev')).toBeVisible()

  const textarea = dialog.locator('textarea')
  await textarea.fill(JSON.stringify({ type: 'object', properties: { orderId: { type: 'string' }, coupon: { type: 'string' } } }))
  await dialog.getByRole('button', { name: 'Preview effective contract' }).click()

  await expect(dialog.getByText('dev V2', { exact: true })).toBeVisible()
  await expect(dialog.getByText('dev M2', { exact: true })).toBeVisible()
  await expect(dialog.getByText('Not registered', { exact: true })).toBeVisible()
  await expect(dialog.getByText('coupon')).toBeVisible()
  await expect(dialog.getByText('The metadata block is platform-owned: it stays read-only here and on every revision.')).toBeVisible()
  await expect(dialog.getByRole('button', { name: 'Append version' })).toBeEnabled()
})

test('domain panel opens the composed-contract modal from the schemas list', async ({ page }) => {
  await installApi(page)
  await page.goto('/domains/orders')

  await expect(page.getByText('dev M2', { exact: true }).first()).toBeVisible()
  await page.getByRole('button', { name: 'Contract', exact: true }).first().click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByText('Composed contract — acme-orders@OrderNotification')).toBeVisible()
  await expect(dialog.getByText('dev V2', { exact: true })).toBeVisible()
})
