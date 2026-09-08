import { test, expect } from './fixtures'

test('gallery renders without credentials or service traffic', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', error => errors.push(error.message))
  await page.goto('/design-system')
  await expect(page.getByRole('heading', { name: 'Design system', exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Open modal', exact: true })).toBeVisible()
  expect(errors).toEqual([])
})

for (const width of [390, 1440]) {
  test(`gallery visual reference at ${width}px`, async ({ page }, testInfo) => {
    await page.setViewportSize({ width, height: 1000 })
    await page.goto('/design-system')
    await expect(page.getByRole('heading', { name: 'Design system', exact: true })).toBeVisible()
    await page.screenshot({ path: testInfo.outputPath(`gallery-${width}.png`), fullPage: true })
  })
}
