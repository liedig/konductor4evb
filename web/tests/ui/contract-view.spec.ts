import { expect, test, type Page } from '@playwright/test'

import { freshState, installApi, signIn } from './api'

/**
 * Schema contract inspection (amended web-ui delta): per-environment badges
 * are selector links that drive one contract read per selection; the catalog
 * detail view shows Schema content (raw business document) and Schema preview
 * (in-force composed document) side by side from the same environment. The
 * console exposes the current pair only — no historical selectors.
 */

function card(page: Page, title: string) {
  return page.locator('section').filter({
    has: page.getByRole('heading', { name: title, exact: true }),
  })
}

test.beforeEach(async ({ page }) => {
  await signIn(page, ['platform-admin'])
})

test('environment indicators are selectable and carry per-environment version badges', async ({ page }) => {
  await installApi(page)
  await page.goto('/catalog/orders/acme-orders%40OrderNotification')

  const environments = card(page, 'Environment')
  const dev = environments.getByRole('button', { name: /dev/ })
  const prod = environments.getByRole('button', { name: /prod/ })
  await expect(dev).toBeVisible()
  await expect(dev.getByText('v2', { exact: true })).toBeVisible()
  await expect(dev.getByText('M2', { exact: true })).toBeVisible()
  await expect(prod.getByText('v3', { exact: true })).toBeVisible()
  await expect(prod.getByText(/M\d/)).toHaveCount(0)

  await expect(dev).toHaveAttribute('aria-pressed', 'true')
  await expect(prod).toHaveAttribute('aria-pressed', 'false')
})

test('both panels load from the selected environment and follow selection changes', async ({ page }) => {
  await installApi(page)
  await page.goto('/catalog/orders/acme-orders%40OrderNotification')

  const content = card(page, 'Schema content')
  const preview = card(page, 'Schema preview')

  // Default selection is the first environment (dev): business v2, M2.
  await expect(content.getByText('The business schema published by orders at dev v2.')).toBeVisible()
  await expect(content.getByText('"orderId"').first()).toBeVisible()
  await expect(preview.getByText(/The contract events in dev validate against right now/)).toBeVisible()
  await expect(preview.getByText(/business v2, metadata M2, digest d{12}/)).toBeVisible()
  await expect(preview.getByText('The metadata block is platform-owned and read-only')).toBeVisible()

  // Switch to prod: business v3 on legacy revision 0 — no digest, no generated fields.
  await card(page, 'Environment').getByRole('button', { name: /prod/ }).click()
  await expect(content.getByText('The business schema published by orders at prod v3.')).toBeVisible()
  await expect(preview.getByText(/The contract events in prod validate against right now/)).toBeVisible()
  await expect(preview.getByText(/business v3, metadata M0/)).toBeVisible()
  await expect(preview.getByText(/digest/)).toHaveCount(0)
})

test('the console exposes no historical pair selectors', async ({ page }) => {
  await installApi(page)
  await page.goto('/catalog/orders/acme-orders%40OrderNotification')

  await expect(card(page, 'Schema preview').getByText(/validate against right now/)).toBeVisible()
  await expect(page.getByLabel('Historical pair')).toHaveCount(0)
  await expect(page.getByLabel('Business version')).toHaveCount(0)
  await expect(page.getByLabel('Metadata revision')).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Load contract' })).toHaveCount(0)
})

test('a failed contract read degrades: registered content stays, preview reports the error', async ({ page }) => {
  // API stage predating the contract route (or a metadata 503): the page
  // must not blank the business document it can still show.
  const state = freshState()
  state.failContract = true
  await installApi(page, state)
  await page.goto('/catalog/orders/acme-orders%40OrderNotification')

  const content = card(page, 'Schema content')
  await expect(content.getByText(/the highest version across environments/)).toBeVisible()
  await expect(content.getByText('"orderId"').first()).toBeVisible()

  const preview = card(page, 'Schema preview')
  await expect(preview.getByRole('alert')).toContainText('Request failed with status 403')
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

test('domain panel links each schema to its catalog detail page instead of a modal', async ({ page }) => {
  await installApi(page)
  await page.goto('/domains/orders')

  await expect(page.getByText('dev M2', { exact: true }).first()).toBeVisible()
  await expect(page.getByRole('button', { name: 'Contract', exact: true })).toHaveCount(0)

  await page.getByRole('link', { name: 'acme-orders@OrderNotification' }).click()
  await expect(page).toHaveURL(/\/catalog\/orders\/acme-orders/)
  await expect(card(page, 'Schema preview').getByText(/validate against right now/)).toBeVisible()
})
