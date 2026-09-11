import { expect, test } from '@playwright/test'

import { installApi, signIn } from './api'

/**
 * Header structure: "All domains" is grouped under Domains as section tabs,
 * Metadata lives under the Settings gear menu, and the identity shows the
 * email local part. Direct-URL journeys never touch the header, so these are
 * the only tests exercising it.
 */

test('platform header groups All domains under Domains and Metadata under Settings', async ({ page }) => {
  await signIn(page, ['platform-admin'])
  await installApi(page)
  await page.goto('/domains')

  const nav = page.getByRole('navigation').first()
  await expect(nav.getByRole('link', { name: 'All domains' })).toHaveCount(0)
  await expect(nav.getByRole('link', { name: 'Metadata' })).toHaveCount(0)

  // The API hands platform roles the full listing, but My domains is an
  // ownership view: it explains instead of rendering those rows as "mine".
  await expect(page.getByRole('heading', { name: 'No owned domains' })).toBeVisible()
  await expect(page.getByRole('link', { name: 'orders', exact: true })).toHaveCount(0)

  // Section tabs appear inside the Domains section and switch views.
  const tabs = page.getByRole('navigation', { name: 'Domains sections' })
  await expect(tabs.getByRole('link', { name: 'My domains' })).toBeVisible()
  await tabs.getByRole('link', { name: 'All domains' }).click()
  await expect(page).toHaveURL(/\/admin\/domains/)
  await expect(page.getByRole('heading', { name: 'All domains' })).toBeVisible()
  await expect(page.getByRole('link', { name: 'orders', exact: true })).toBeVisible()
  await expect(nav.getByRole('link', { name: 'Domains' })).toHaveClass(/bg-surface-raised/)

  // The Settings gear opens the menu; Metadata navigates and closes it.
  const gear = page.getByRole('button', { name: 'Settings' })
  await gear.click()
  const menu = page.getByRole('menu')
  await expect(menu).toBeVisible()
  await menu.getByRole('menuitem', { name: 'Metadata' }).click()
  await expect(page).toHaveURL(/\/admin\/metadata/)
  await expect(page.getByRole('menu')).toHaveCount(0)
  await expect(gear).toHaveClass(/bg-surface-raised/)

  // Identity shows the local part; the full address is tooltip-only. (Page
  // content like revision history still shows full emails — scope to the banner.)
  const banner = page.getByRole('banner')
  await expect(banner.getByText('platform-admin@example.com')).toHaveCount(0)
  // The identity span (title = full address) shows the local part only.
  await expect(banner.getByTitle('platform-admin@example.com')).toHaveText('platform-admin')
})

test('settings menu closes on Escape and on outside pointer', async ({ page }) => {
  await signIn(page, ['platform-admin'])
  await installApi(page)
  await page.goto('/domains')

  const gear = page.getByRole('button', { name: 'Settings' })
  await gear.click()
  await expect(page.getByRole('menu')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('menu')).toHaveCount(0)
  await expect(gear).toBeFocused()

  await gear.click()
  await page.getByRole('heading', { name: 'My domains' }).click()
  await expect(page.getByRole('menu')).toHaveCount(0)
})

test('team users see neither the gear nor the section tabs', async ({ page }) => {
  await signIn(page, ['team-orders'], 'owner@example.com')
  await installApi(page)
  await page.goto('/domains')

  await expect(page.getByRole('button', { name: 'Settings' })).toHaveCount(0)
  await expect(page.getByRole('navigation', { name: 'Domains sections' })).toHaveCount(0)
  await expect(page.getByText('owner', { exact: true }).first()).toBeVisible()

  // Team users keep their ownership list on the same screen.
  await expect(page.getByRole('link', { name: 'orders', exact: true })).toBeVisible()
})
