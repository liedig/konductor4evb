import type { Page } from '@playwright/test'

import { expect, test } from './fixtures'
import { installApi, signIn } from './api'

/**
 * The domain detail page's audience panels — outbound subscriptions (who
 * consumes this domain's schemas) and access grants (who may) — summarize:
 * the five most recently created entries, newest first, with a link to
 * dedicated views listing every entry. The dedicated grants view keeps the
 * full management surface: granting and revoking work there too.
 */

function card(page: Page, title: string) {
  return page.locator('section').filter({
    has: page.getByRole('heading', { name: title, exact: true }),
  })
}

function fulfillJson(route: import('@playwright/test').Route, body: unknown) {
  return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })
}

/** Consumer domains created on consecutive October days: index 0 on the
 * 20th … index 6 on the 26th. The newest sorts first everywhere. */
const CONSUMERS = ['alpha', 'bravo', 'charlie', 'delta', 'echo', 'foxtrot', 'golf']

function subscriptionFixture(consumer: string, index: number) {
  return {
    subscriptionId: `sub-${consumer}`,
    consumerDomain: consumer,
    producerDomain: 'orders',
    schemaName: 'acme-orders@OrderPlaced',
    status: 'ACTIVE',
    environments: { dev: { status: 'ACTIVE', enabled: true, pinnedVersion: 2 } },
    createdAt: `2026-10-${20 + index}T09:00:00.000Z`,
  }
}

function entitlementFixture(consumer: string, index: number) {
  return {
    entitlementId: `ent-${consumer}`,
    consumerDomain: consumer,
    producerDomain: 'orders',
    scopePattern: 'OrderPlaced',
    createdAt: `2026-10-${20 + index}T09:00:00.000Z`,
    createdBy: 'sliedig+konductoradmin@example.com',
  }
}

test('the outbound subscriptions panel caps at the five newest consumers and links to the full view', async ({ page }) => {
  await signIn(page, ['platform-admin'])
  await installApi(page)
  await page.route('**/api/dev/subscriptions**', route =>
    fulfillJson(route, { items: CONSUMERS.map(subscriptionFixture) }))
  await page.goto('/domains/orders')

  const panel = card(page, 'Outbound subscriptions')
  const rows = panel.getByRole('row')
  await expect(rows).toHaveCount(6)
  await expect(rows.nth(1)).toContainText('golf')
  await expect(panel.getByText('bravo', { exact: true })).toHaveCount(0)
  await expect(panel.getByText('alpha', { exact: true })).toHaveCount(0)

  const viewAll = panel.getByRole('link', { name: 'View all subscriptions' })
  await expect(viewAll).toBeVisible()
  await expect(viewAll).toHaveAttribute('href', '/domains/orders/subscriptions')
})

test('the dedicated subscriptions view lists every consumer', async ({ page }) => {
  await signIn(page, ['platform-admin'])
  await installApi(page)
  await page.route('**/api/dev/subscriptions**', route =>
    fulfillJson(route, { items: CONSUMERS.map(subscriptionFixture) }))
  await page.goto('/domains/orders/subscriptions')

  await expect(page.getByRole('heading', { name: 'Outbound subscriptions', exact: true })).toBeVisible()
  const rows = page.getByRole('row')
  await expect(rows).toHaveCount(8)
  await expect(rows.nth(1)).toContainText('golf')
  await expect(rows.nth(7)).toContainText('alpha')
  await expect(page.getByRole('link', { name: 'View all subscriptions' })).toHaveCount(0)
})

test('the access grants panel caps at the five newest grants and links to the full view', async ({ page }) => {
  await signIn(page, ['platform-admin'])
  await installApi(page)
  await page.route('**/api/dev/entitlements**', route =>
    fulfillJson(route, { entitlements: CONSUMERS.map(entitlementFixture) }))
  await page.goto('/domains/orders')

  const panel = card(page, 'Access grants')
  const rows = panel.getByRole('row')
  await expect(rows).toHaveCount(6)
  await expect(rows.nth(1)).toContainText('golf')
  await expect(panel.getByText('bravo', { exact: true })).toHaveCount(0)
  await expect(panel.getByText('alpha', { exact: true })).toHaveCount(0)
  await expect(panel.getByRole('button', { name: 'Revoke' })).toHaveCount(5)

  const viewAll = panel.getByRole('link', { name: 'View all grants' })
  await expect(viewAll).toBeVisible()
  await expect(viewAll).toHaveAttribute('href', '/domains/orders/grants')
})

test('the dedicated grants view lists every grant and manages access', async ({ page }) => {
  await signIn(page, ['platform-admin'])
  await installApi(page)
  const grants = CONSUMERS.map(entitlementFixture)
  await page.route('**/api/dev/entitlements**', (route) => {
    const request = route.request()
    if (request.method() === 'DELETE') {
      const id = decodeURIComponent(request.url().split('/entitlements/')[1] ?? '')
      const index = grants.findIndex(grant => grant.entitlementId === id)
      if (index >= 0) grants.splice(index, 1)
      return fulfillJson(route, {})
    }
    if (request.method() === 'POST') {
      const body = JSON.parse(request.postData() ?? '{}') as { consumerDomain?: string, scopePattern?: string }
      grants.unshift({
        entitlementId: 'ent-new-grant',
        consumerDomain: body.consumerDomain,
        producerDomain: 'orders',
        scopePattern: body.scopePattern,
        createdAt: '2026-10-27T09:00:00.000Z',
        createdBy: 'sliedig+konductoradmin@example.com',
      })
      return fulfillJson(route, {})
    }
    return fulfillJson(route, { entitlements: grants })
  })
  await page.route('**/api/dev/domains?view=directory*', route =>
    fulfillJson(route, {
      domains: [
        { domainName: 'alpha', status: 'ACTIVE' },
        { domainName: 'bravo', status: 'ACTIVE' },
      ],
    }))
  await page.goto('/domains/orders/grants')

  await expect(page.getByRole('heading', { name: 'Access grants', exact: true })).toBeVisible()
  const rows = page.getByRole('row')
  await expect(rows).toHaveCount(8)
  await expect(page.getByRole('button', { name: 'Grant access' })).toBeVisible()

  await rows.nth(1).getByRole('button', { name: 'Revoke' }).click()
  const confirm = page.getByRole('alertdialog')
  await expect(confirm.getByText('Revoke access for golf')).toBeVisible()
  await confirm.getByRole('button', { name: 'Revoke', exact: true }).click()
  await expect(page.getByText('Access revoked for golf', { exact: true })).toBeVisible()
  await expect(rows).toHaveCount(7)
  await expect(page.getByText('golf', { exact: true })).toHaveCount(0)

  await page.getByRole('button', { name: 'Grant access' }).click()
  await expect(page.getByRole('heading', { name: 'Grant access', exact: true })).toBeVisible()
  await page.getByLabel('Consumer domain').selectOption('alpha')
  await page.getByLabel('Covered schemas').selectOption('OrderNotification')
  await page.getByRole('button', { name: 'Grant', exact: true }).click()
  await expect(page.getByText('Access granted to alpha', { exact: true })).toBeVisible()
  await expect(rows).toHaveCount(8)
  await expect(rows.nth(1)).toContainText('alpha')
})

test('the audience panels skip the view-all links when everything fits', async ({ page }) => {
  await signIn(page, ['platform-admin'])
  await installApi(page)
  await page.route('**/api/dev/subscriptions**', route =>
    fulfillJson(route, { items: CONSUMERS.slice(0, 3).map(subscriptionFixture) }))
  await page.route('**/api/dev/entitlements**', route =>
    fulfillJson(route, { entitlements: CONSUMERS.slice(0, 2).map(entitlementFixture) }))
  await page.goto('/domains/orders')

  await expect(card(page, 'Outbound subscriptions').getByRole('row')).toHaveCount(4)
  await expect(card(page, 'Access grants').getByRole('row')).toHaveCount(3)
  await expect(page.getByRole('link', { name: 'View all subscriptions' })).toHaveCount(0)
  await expect(page.getByRole('link', { name: 'View all grants' })).toHaveCount(0)
})
