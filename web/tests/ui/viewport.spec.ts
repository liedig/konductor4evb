import { expect, test, type Page } from '@playwright/test'

import { installApi, signIn } from './api'

/**
 * Narrow viewports. 375px stands in for both a phone and a 200%-zoomed
 * desktop window (the layout viewport halves); true pinch-zoom behavior is
 * outside an automated layout check and is recorded as a manual gap.
 */

function card(page: Page, title: string) {
  return page.locator('section').filter({
    has: page.getByRole('heading', { name: title, exact: true }),
  })
}

test.beforeEach(async ({ page }) => {
  await signIn(page, ['platform-admin'])
  await installApi(page)
  await page.setViewportSize({ width: 375, height: 667 })
})

test('metadata administration stays operable at 375px; the horizontal overflow is a recorded gap', async ({ page }) => {
  await page.goto('/admin/metadata')

  await expect(page.getByRole('heading', { name: 'Metadata administration' })).toBeVisible()

  // Recorded accessibility gap (not marked passed): the admin page currently
  // overflows horizontally at 375px — the fixed-width editor columns and the
  // environment table need a responsive pass. The controls remain operable.
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  )
  test.info().annotations.push({ type: 'note', description: `375px horizontal overflow: ${overflow}px` })

  const inspect = page.getByRole('button', { name: 'Inspect revision M2' })
  await inspect.focus()
  await expect(inspect).toBeFocused()
  await inspect.click()
  await expect(page.getByRole('dialog').getByText('Revision M2')).toBeVisible()
})

test('catalog detail remains readable and contract-switchable at 375px', async ({ page }) => {
  await page.goto('/catalog/orders/acme-orders%40OrderNotification')

  const contract = card(page, 'Composed contract')
  await expect(contract.getByText('dev V2', { exact: true })).toBeVisible()
  await contract.getByRole('button', { name: 'Inherited metadata' }).click()
  await expect(contract.getByText('read-only for every caller')).toBeVisible()
})

test('the domain panel actions remain reachable at 375px', async ({ page }) => {
  await page.goto('/domains/orders')

  await page.getByRole('button', { name: 'New version' }).first().click()
  await expect(page.getByRole('dialog').getByText('Target environment: dev')).toBeVisible()
})
