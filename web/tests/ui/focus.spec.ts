import { expect, test } from '@playwright/test'

import { installApi, signIn } from './api'

/**
 * Keyboard and dialog focus on the metadata admin flows: Escape dismissal,
 * keyboard activation, dialog roles. NxModal's focus trap and
 * restore-to-opener come from the Reka Dialog primitives and are covered on
 * the gallery (tests/ui/overlays.spec.ts); these specs assert the admin
 * flows wire them up correctly.
 */

test.beforeEach(async ({ page }) => {
  await signIn(page, ['platform-admin'])
})

test('activation controls are keyboard-operable end to end', async ({ page }) => {
  await installApi(page)
  await page.goto('/admin/metadata')

  const prepare = page.getByRole('button', { name: 'Prepare activation for prod' })
  await prepare.focus()
  await page.keyboard.press('Enter')
  const dialog = page.getByRole('dialog')
  await expect(dialog).toBeVisible()
  await expect(dialog).toHaveAttribute('aria-modal', 'true')

  await dialog.getByRole('button', { name: 'Continue to activation' }).focus()
  await page.keyboard.press('Enter')
  const confirm = page.getByRole('dialog').filter({ hasText: 'Activate M2 for prod' })
  await expect(confirm).toBeVisible()

  await confirm.getByRole('checkbox').focus()
  await page.keyboard.press('Space')
  await confirm.getByRole('button', { name: 'Activate M2 for prod' }).focus()
  await page.keyboard.press('Enter')
  await expect(page.getByText('prod is now active at M2', { exact: true })).toBeVisible()
})

test('Escape closes the prepare dialog while idle', async ({ page }) => {
  await installApi(page)
  await page.goto('/admin/metadata')

  await page.getByRole('button', { name: 'Prepare activation for prod' }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
})

test('Escape during an in-flight activation is refused', async ({ page }) => {
  const state = await installApi(page)
  state.activationDelayMs = 1200
  await page.goto('/admin/metadata')

  await page.getByRole('button', { name: 'Prepare activation for prod' }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByRole('button', { name: 'Continue to activation' }).click()
  const confirm = page.getByRole('dialog').filter({ hasText: 'Activate M2 for prod' })
  await confirm.getByRole('checkbox').check()
  await confirm.getByRole('button', { name: 'Activate M2 for prod' }).click()

  await expect(confirm.getByText('Activating…')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(confirm).toBeVisible()
  await expect(confirm).toBeHidden({ timeout: 10_000 })
})

test('the inspect dialog opens via keyboard and closes with Escape', async ({ page }) => {
  await installApi(page)
  await page.goto('/admin/metadata')

  const inspect = page.getByRole('button', { name: 'Inspect revision M2' })
  await inspect.focus()
  await page.keyboard.press('Enter')
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByText('Revision M2')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
})
