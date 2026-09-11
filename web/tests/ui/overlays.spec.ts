import { test, expect } from './fixtures'

test.beforeEach(async ({ page }) => {
  await page.goto('/design-system')
})

test('modal contains focus, isolates background and restores the opener', async ({ page }) => {
  const opener = page.getByRole('button', { name: 'Open modal', exact: true })
  await opener.click()
  const dialog = page.getByRole('dialog', { name: 'Modal', exact: true })
  await expect(dialog).toBeVisible()
  await expect(dialog).toHaveAttribute('aria-modal', 'true')
  await expect(page.getByLabel('Demo field')).toBeFocused()
  for (let index = 0; index < 6; index++) {
    await page.keyboard.press(index % 2 ? 'Shift+Tab' : 'Tab')
    expect(await dialog.evaluate(el => el.contains(document.activeElement))).toBe(true)
  }
  await expect(opener).toHaveCount(0)
  await page.keyboard.press('Escape')
  await expect(dialog).not.toBeVisible()
  await expect(opener).toBeFocused()
  await opener.click()
  await page.getByRole('button', { name: 'Close', exact: true }).click()
  await expect(opener).toBeFocused()
  await opener.click()
  // Wait for the dialog to be fully mounted before the click-away so the
  // overlay's dismissal handler is in place.
  await expect(dialog).toBeVisible()
  await page.mouse.click(5, 5)
  await expect(dialog).not.toBeVisible()
  await expect(opener).toBeFocused()
})

test('removed opener has a logical focus fallback', async ({ page }) => {
  await page.getByRole('button', { name: 'Removed opener demo' }).click()
  await page.getByRole('button', { name: 'Remove opening control' }).click()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('heading', { name: 'Design system', exact: true })).toBeFocused()
})

test('long modal keeps actions reachable on narrow screens', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 600 })
  await page.getByRole('button', { name: 'Long modal', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: 'Long modal' })
  const bounds = await dialog.boundingBox()
  expect(bounds!.width).toBeLessThan(390)
  expect(bounds!.height).toBeLessThan(600)
  await page.getByRole('button', { name: 'Finish reading' }).click()
  await expect(dialog).not.toBeVisible()
})

test('confirmation cancellation is explicit and busy actions cannot repeat or dismiss', async ({ page }) => {
  await page.getByRole('button', { name: 'Open confirm' }).click()
  const dialog = page.getByRole('alertdialog', { name: 'Disable domain' })
  await expect(dialog).toBeVisible()
  await expect(dialog).toHaveAccessibleDescription(/Disabling rejects/)
  await expect(dialog.getByRole('button', { name: 'Cancel' })).toBeFocused()
  await page.mouse.click(5, 5)
  await expect(dialog).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(dialog).not.toBeVisible()
  await page.getByRole('button', { name: 'Open confirm' }).click()
  await dialog.getByRole('button', { name: 'Disable', exact: true }).click()
  await expect(dialog.getByRole('button', { name: 'Disable', exact: true })).toBeDisabled()
  await page.keyboard.press('Enter')
  await page.keyboard.press('Escape')
  await page.mouse.click(5, 5)
  await expect(dialog).toBeVisible()
  await expect(page.getByText('Demo attempts: 1.', { exact: false })).toBeVisible()
  await expect(dialog.getByText('Demo failed. Try again or cancel.')).toBeVisible()
  await dialog.getByRole('button', { name: 'Disable', exact: true }).click()
  await expect(page.getByText('Demo attempts: 2.', { exact: false })).toBeVisible()
  await expect(dialog.getByRole('button', { name: 'Cancel' })).toBeEnabled()
  await dialog.getByRole('button', { name: 'Cancel' }).click()
  await expect(dialog).not.toBeVisible()
})
