import { test, expect } from './fixtures'

test.beforeEach(async ({ page }) => {
  await page.goto('/design-system')
})

test('controls keep names, descriptions, required and disabled semantics', async ({ page }) => {
  await expect(page.getByLabel('Domain name')).toHaveAccessibleDescription('Lowercase letters, digits, and hyphens.')
  await expect(page.getByLabel('With error', { exact: true })).toHaveAccessibleDescription('Domain name is required.')
  await expect(page.getByLabel('With error', { exact: true })).toHaveAttribute('aria-invalid', 'true')
  await expect(page.getByLabel('Select with error')).toHaveAccessibleDescription('Choose an environment.')
  await expect(page.getByLabel('Disabled input')).toBeDisabled()
  await expect(page.getByLabel('Disabled select')).toBeDisabled()
  await expect(page.getByRole('button', { name: 'Loading', exact: true })).toBeDisabled()
  await expect(page.getByRole('button', { name: 'Loading', exact: true })).toHaveAttribute('aria-busy', 'true')
  expect(await page.getByLabel('Owning team').evaluate((select: HTMLSelectElement) => select.validity.valueMissing)).toBe(true)
  await page.getByLabel('Environment filter').selectOption('dev')
  await page.getByLabel('Environment filter').selectOption('')
  await expect(page.getByLabel('Environment filter')).toHaveValue('')
})
