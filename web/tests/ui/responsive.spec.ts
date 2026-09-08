import { test, expect } from './fixtures'

test('overlay and toast remain usable at 200 percent text size with reduced motion', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 390, height: 800 })
  await page.emulateMedia({ reducedMotion: 'reduce' })
  await page.goto('/design-system')
  await page.evaluate(() => { document.documentElement.style.fontSize = '200%' })
  await page.getByRole('button', { name: 'Long modal', exact: true }).click()
  const dialog = page.getByRole('dialog', { name: 'Long modal' })
  const bounds = await dialog.boundingBox()
  expect(bounds!.x).toBeGreaterThanOrEqual(0)
  expect(bounds!.x + bounds!.width).toBeLessThanOrEqual(390)
  await page.getByRole('button', { name: 'Finish reading' }).scrollIntoViewIfNeeded()
  await page.screenshot({ path: testInfo.outputPath('modal-large-text.png') })
  await page.getByRole('button', { name: 'Finish reading' }).click()
  await page.getByRole('button', { name: 'Long notification', exact: true }).click()
  const toast = page.locator('[data-state="open"]')
  await expect(toast).toBeVisible()
  const toastBounds = await toast.boundingBox()
  expect(toastBounds!.x).toBeGreaterThanOrEqual(0)
  expect(toastBounds!.x + toastBounds!.width).toBeLessThanOrEqual(390)
  await page.getByRole('button', { name: 'Dismiss notification' }).scrollIntoViewIfNeeded()
  await page.screenshot({ path: testInfo.outputPath('toast-large-text.png') })
  await page.getByRole('button', { name: 'Dismiss notification' }).click()
  await expect(toast).toHaveCount(0)
  const transition = await page.getByRole('button', { name: 'Success', exact: true }).evaluate(el => getComputedStyle(el).transitionDuration)
  expect(parseFloat(transition)).toBeLessThan(0.01)
})

test('gallery controls retain visible keyboard focus and semantic tones', async ({ page }, testInfo) => {
  await page.goto('/design-system')
  await page.getByRole('button', { name: 'Primary', exact: true }).focus()
  await page.keyboard.press('Tab')
  const button = page.getByRole('button', { name: 'Secondary', exact: true })
  await expect(button).toBeFocused()
  const outline = await button.evaluate(el => {
    const style = getComputedStyle(el)
    return { width: style.outlineWidth, style: style.outlineStyle }
  })
  expect(outline).toEqual({ width: '2px', style: 'solid' })
  await expect(page.getByText('AWAITING PROMOTION', { exact: true })).toHaveClass(/text-warning/)
  await expect(page.getByText('FAILED', { exact: true })).toHaveClass(/text-danger/)
  await page.screenshot({ path: testInfo.outputPath('keyboard-focus.png') })
})

test('protected consumer routes still require sign-in', async ({ page }) => {
  for (const route of ['/domains/example', '/subscriptions', '/admin/domains', '/catalog/example/Example']) {
    await page.goto(route)
    await expect(page).toHaveURL(/\/login$/)
  }
})
