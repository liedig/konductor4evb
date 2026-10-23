import type { Page } from '@playwright/test'

import { expect, test } from './fixtures'
import { installApi, signIn } from './api'

/**
 * Per-environment account overrides across the three console surfaces:
 *   Request form  — one required account field per configured environment
 *     (dev/staging/prod come from the config.json interception in
 *     fixtures.ts, not from a deployed stack),
 *     12-digit validation, complete payload (every environment is pinned;
 *     the owner account is derived from the authoring environment).
 *   Approval card — the pending-registration inbox shows the resolved
 *     env → account map so approvers see where resources would land.
 *   Domain detail — the Overview card renders the effective map, and
 *     console links resolve against each environment's account (an
 *     override env's app bus lives in the override account, so the link
 *     must not be dropped by the owner-account comparison).
 */

const OWNER_ACCOUNT = '111122223333'
const OVERRIDE_ACCOUNT = '999900001111'
const REGION = 'ap-southeast-2'

function registration(overrides: Record<string, unknown> = {}) {
  return {
    domainName: 'orders',
    description: 'Order lifecycle events',
    ownerAccountId: OWNER_ACCOUNT,
    ownerEmail: 'owner@example.com',
    secondaryOwnerEmails: ['second@example.com'],
    owningTeam: 'team-orders',
    createdAt: '2026-09-20T09:00:00.000Z',
    environmentAccounts: { dev: OWNER_ACCOUNT, staging: OWNER_ACCOUNT, prod: OVERRIDE_ACCOUNT },
    ...overrides,
  }
}

function domainDetail(overrides: Record<string, unknown> = {}) {
  return {
    domainName: 'orders',
    description: 'Order lifecycle events',
    ownerAccountId: OWNER_ACCOUNT,
    ownerEmail: 'owner@example.com',
    status: 'ACTIVE',
    environments: ['dev', 'prod'],
    environmentAccounts: { dev: OWNER_ACCOUNT, prod: OVERRIDE_ACCOUNT },
    resourceArns: {
      dev: {
        appEventBusArn: `arn:aws:events:${REGION}:${OWNER_ACCOUNT}:event-bus/acme-dev-orders`,
      },
      prod: {
        appEventBusArn: `arn:aws:events:${REGION}:${OVERRIDE_ACCOUNT}:event-bus/acme-prod-orders`,
      },
    },
    createdAt: '2026-08-01T09:00:00.000Z',
    ...overrides,
  }
}

/** Overrides the pending-registrations and describe routes after installApi (last route registered wins). */
async function routeJson(page: Page, glob: string, body: unknown) {
  await page.route(glob, async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(body),
    })
  })
}

test('request form renders one required account field per configured environment', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 })
  await signIn(page, ['team-orders'], 'owner@example.com')
  await installApi(page)
  await page.goto('/domains/new')

  await expect(page.getByText('Per-environment accounts')).toBeVisible()
  await expect(page.getByRole('textbox', { name: 'dev account ID' })).toBeVisible()
  await expect(page.getByRole('textbox', { name: 'staging account ID' })).toBeVisible()
  await expect(page.getByRole('textbox', { name: 'prod account ID' })).toBeVisible()
  await expect(page.getByRole('textbox', { name: 'Owner AWS account ID' })).toHaveCount(0)
})

test('request form requires every environment account and submits the complete map', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 })
  await signIn(page, ['team-orders'], 'owner@example.com')
  await installApi(page)

  let captured: Record<string, unknown> | null = null
  await page.route('**/api/dev/domains', async (route) => {
    if (route.request().method() !== 'POST') return route.fallback()
    captured = route.request().postDataJSON() as Record<string, unknown>
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ requestId: 'req-1' }),
    })
  })

  await page.goto('/domains/new')
  await page.getByRole('textbox', { name: 'Domain name' }).fill('orders')
  await page.getByRole('textbox', { name: 'Owner email *', exact: true }).fill('owner@example.com')
  await page.getByRole('textbox', { name: 'Secondary owner emails' }).fill('second@example.com')
  await page.getByRole('textbox', { name: 'dev account ID' }).fill(OWNER_ACCOUNT)
  await page.getByRole('textbox', { name: 'staging account ID' }).fill(OWNER_ACCOUNT)

  const prodField = page.getByRole('textbox', { name: 'prod account ID' })
  await page.getByRole('button', { name: 'Submit request' }).click()
  await expect(page.getByText('A 12-digit AWS account ID.', { exact: true })).toHaveCount(1)
  expect(captured).toBeNull()

  await prodField.fill('123')
  await page.getByRole('button', { name: 'Submit request' }).click()
  await expect(page.getByText('A 12-digit AWS account ID.', { exact: true })).toHaveCount(1)
  expect(captured).toBeNull()

  await prodField.fill(OVERRIDE_ACCOUNT)
  await page.getByRole('button', { name: 'Submit request' }).click()
  await expect(page.getByText('Registration request accepted', { exact: true })).toBeVisible()
  await expect(page.getByRole('heading', { name: 'Request received' })).toBeVisible()
  expect(captured).not.toBeNull()
  expect(captured!['environmentAccounts']).toEqual({
    dev: OWNER_ACCOUNT,
    staging: OWNER_ACCOUNT,
    prod: OVERRIDE_ACCOUNT,
  })
  expect(captured!['ownerAccountId']).toBe(OWNER_ACCOUNT)
})

test('approval card shows the resolved account per environment', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 })
  await signIn(page, ['platform-admin'])
  await installApi(page)
  await routeJson(page, '**/api/dev/domains/registrations/pending**', { registrations: [registration()] })
  await page.goto('/admin')

  const card = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'orders', exact: true }),
  })
  await expect(card.getByText('Accounts', { exact: true })).toBeVisible()
  await expect(card.getByText(`dev → ${OWNER_ACCOUNT}`)).toBeVisible()
  await expect(card.getByText(`staging → ${OWNER_ACCOUNT}`)).toBeVisible()
  await expect(card.getByText(`prod → ${OVERRIDE_ACCOUNT}`)).toBeVisible()
})

test('approval card omits the accounts row for registrations without the field', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 })
  await signIn(page, ['platform-admin'])
  await installApi(page)
  await routeJson(page, '**/api/dev/domains/registrations/pending**', {
    registrations: [registration({ environmentAccounts: undefined })],
  })
  await page.goto('/admin')

  const card = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'orders', exact: true }),
  })
  await expect(card.getByRole('heading', { name: 'orders', exact: true })).toBeVisible()
  await expect(card.getByText('Accounts', { exact: true })).toHaveCount(0)
})

test('domain detail renders the effective map and links resources in override accounts', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 })
  await signIn(page, ['platform-admin'])
  await installApi(page)
  await routeJson(page, '**/api/dev/domains/orders', domainDetail())
  await routeJson(page, '**/api/dev/schemas**', { items: [] })
  await routeJson(page, '**/api/dev/subscriptions**', { items: [] })
  await page.goto('/domains/orders')

  const overview = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Overview', exact: true }),
  })
  await expect(overview.getByText('Accounts per environment')).toBeVisible()
  await expect(overview.getByText(`dev → ${OWNER_ACCOUNT}`)).toBeVisible()
  await expect(overview.getByText(`prod → ${OVERRIDE_ACCOUNT}`)).toBeVisible()

  // The override environment's app bus lives in the override account; the
  // console link must resolve against that account, not the owner account.
  await page.getByRole('button', { name: 'prod', exact: true }).click()
  const panel = page.locator('#env-panel-prod')
  await expect(panel).toBeVisible()
  const link = panel.getByRole('link')
  await expect(link).toHaveCount(1)
  await expect(link).toHaveAttribute('href', /console\.aws\.amazon\.com\/events/)
})

test('domain detail without the field keeps owner-account console links', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 })
  await signIn(page, ['platform-admin'])
  await installApi(page)
  await routeJson(page, '**/api/dev/domains/orders', domainDetail({
    environmentAccounts: undefined,
    resourceArns: {
      dev: { appEventBusArn: `arn:aws:events:${REGION}:${OWNER_ACCOUNT}:event-bus/acme-dev-orders` },
    },
  }))
  await routeJson(page, '**/api/dev/schemas**', { items: [] })
  await routeJson(page, '**/api/dev/subscriptions**', { items: [] })
  await page.goto('/domains/orders')

  const overview = page.locator('section').filter({
    has: page.getByRole('heading', { name: 'Overview', exact: true }),
  })
  await expect(overview.getByText('Accounts per environment')).toHaveCount(0)

  await page.getByRole('button', { name: 'dev', exact: true }).click()
  const panel = page.locator('#env-panel-dev')
  await expect(panel.getByRole('link')).toHaveCount(1)
})
