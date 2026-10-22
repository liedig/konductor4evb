import type { Page } from '@playwright/test'
import { expect } from './fixtures'

export async function openGalleryWithClock(page: Page) {
  await page.clock.install({ time: new Date('2026-01-01T00:00:00Z') })
  await page.goto('/design-system')
  await expect(page.getByRole('heading', { name: 'Design system', exact: true })).toBeVisible()
  await page.clock.pauseAt(new Date('2026-01-02T00:00:00Z'))
}
