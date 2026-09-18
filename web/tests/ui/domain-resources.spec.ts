import { expect, test } from './fixtures'
import type { Page } from '@playwright/test'

import { installApi, signIn } from './api'

/**
 * Ownership-aware resource references on the domain detail page.
 *
 * Six provisioned resource fields are split into two groups:
 *   Platform-managed: platformEventBusArn, schemaRegistryArn,
 *     subscriptionDlqArn, dashboardArn (always plain text, never linked)
 *   Resources in your account: appEventBusArn, integrationDlqArn
 *     (linked only when the identifier's account matches ownerAccountId)
 *
 * Malformed, missing, unknown, or account-mismatched identifiers degrade
 * to plain text. All identifiers remain visible regardless of link status.
 */

const OWNER_ACCOUNT = '111122223333'
const PLATFORM_ACCOUNT = '999988887777'
const REGION = 'ap-southeast-2'

function arn(service: string, resource: string, account = OWNER_ACCOUNT): string {
  return `arn:aws:${service}:${REGION}:${account}:${resource}`
}

/** Full set of resource identifiers for one environment. */
function fullResourceArns(account = OWNER_ACCOUNT) {
  return {
    platformEventBusArn: arn('events', 'event-bus/acme-prod-platform', PLATFORM_ACCOUNT),
    appEventBusArn: arn('events', 'event-bus/acme-prod-orders', account),
    schemaRegistryArn: arn('schemas', 'registry/acme-prod-schemas', PLATFORM_ACCOUNT),
    subscriptionDlqArn: arn('sqs', 'acme-prod-subscription-dlq', PLATFORM_ACCOUNT),
    integrationDlqArn: `https://sqs.${REGION}.amazonaws.com/${account}/acme-prod-integration-dlq`,
    dashboardArn: arn('cloudwatch', 'dashboard/acme-prod-dashboard', PLATFORM_ACCOUNT),
  }
}

function domainDetail(overrides: Record<string, unknown> = {}) {
  return {
    domainName: 'orders',
    description: 'Order lifecycle events',
    ownerAccountId: OWNER_ACCOUNT,
    ownerEmail: 'owner@example.com',
    status: 'ACTIVE',
    environments: ['dev'],
    resourceArns: { dev: fullResourceArns() },
    createdAt: '2026-08-01T09:00:00.000Z',
    ...overrides,
  }
}

async function installDomainApi(page: Page, detail: Record<string, unknown>) {
  await page.route('**/dev/domains/orders', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(detail),
    })
  })
  await page.route('**/dev/schemas**', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ items: [] }),
    })
  })
  await page.route('**/dev/subscriptions**', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ items: [] }),
    })
  })
}

/** Expand the environment collapsible and return the panel. */
async function envPanel(page: Page, env = 'dev') {
  const toggle = page.getByRole('button', { name: env, exact: true })
  await expect(toggle).toBeVisible()
  await toggle.click()
  const panel = page.locator(`#env-panel-${env}`)
  await expect(panel).toBeVisible()
  return panel
}

test.beforeEach(async ({ page }) => {
  await signIn(page, ['platform-admin'])
  await installApi(page)
})

test.describe('mixed accounts (platform != domain)', () => {
  test('platform resources are visible without links; domain resources are linked', async ({ page }) => {
    await installDomainApi(page, domainDetail())
    await page.goto('/domains/orders')

    const panel = await envPanel(page)

    // Platform-managed resources: visible, no links
    const platformGroup = panel.locator('[data-resource-group="platform"]')
    await expect(platformGroup).toBeVisible()
    await expect(platformGroup.getByText('platformEventBusArn')).toBeVisible()
    await expect(platformGroup.getByText(/acme-prod-platform/)).toBeVisible()
    await expect(platformGroup.getByText('schemaRegistryArn')).toBeVisible()
    await expect(platformGroup.getByText(/acme-prod-schemas/)).toBeVisible()
    await expect(platformGroup.getByText('subscriptionDlqArn')).toBeVisible()
    await expect(platformGroup.getByText(/acme-prod-subscription-dlq/)).toBeVisible()
    await expect(platformGroup.getByText('dashboardArn')).toBeVisible()
    await expect(platformGroup.getByText(/acme-prod-dashboard/)).toBeVisible()
    await expect(platformGroup.locator('a')).toHaveCount(0)

    // Resources in your account: visible, linked
    const domainGroup = panel.locator('[data-resource-group="domain"]')
    await expect(domainGroup).toBeVisible()
    await expect(domainGroup.getByText('appEventBusArn')).toBeVisible()
    const busLink = domainGroup.locator('a').filter({ hasText: 'acme-prod-orders' })
    await expect(busLink).toBeVisible()
    await expect(busLink).toHaveAttribute('href', /console\.aws\.amazon\.com\/events/)

    await expect(domainGroup.getByText('integrationDlqArn')).toBeVisible()
    const dlqLink = domainGroup.locator('a').filter({ hasText: 'acme-prod-integration-dlq' })
    await expect(dlqLink).toBeVisible()
    await expect(dlqLink).toHaveAttribute('href', /console\.aws\.amazon\.com\/sqs/)
  })

  test('group headings are visible', async ({ page }) => {
    await installDomainApi(page, domainDetail())
    await page.goto('/domains/orders')

    const panel = await envPanel(page)
    await expect(panel.getByText('Platform-managed resources')).toBeVisible()
    await expect(panel.getByText('Resources in your account')).toBeVisible()
  })
})

test.describe('co-located accounts (platform == domain)', () => {
  test('platform resources remain plain text even when accounts match', async ({ page }) => {
    const coLocated = fullResourceArns(OWNER_ACCOUNT)
    coLocated.platformEventBusArn = arn('events', 'event-bus/acme-prod-platform', OWNER_ACCOUNT)
    coLocated.schemaRegistryArn = arn('schemas', 'registry/acme-prod-schemas', OWNER_ACCOUNT)
    coLocated.subscriptionDlqArn = arn('sqs', 'acme-prod-subscription-dlq', OWNER_ACCOUNT)
    coLocated.dashboardArn = arn('cloudwatch', 'dashboard/acme-prod-dashboard', OWNER_ACCOUNT)

    await installDomainApi(page, domainDetail({
      resourceArns: { dev: coLocated },
    }))
    await page.goto('/domains/orders')

    const panel = await envPanel(page)

    // Platform group: no links at all despite matching account
    const platformGroup = panel.locator('[data-resource-group="platform"]')
    await expect(platformGroup).toBeVisible()
    await expect(platformGroup.locator('a')).toHaveCount(0)
    // Identifiers still visible
    await expect(platformGroup.getByText(/acme-prod-platform/)).toBeVisible()
    await expect(platformGroup.getByText(/acme-prod-schemas/)).toBeVisible()

    // Domain group: links still present
    const domainGroup = panel.locator('[data-resource-group="domain"]')
    await expect(domainGroup.locator('a')).toHaveCount(2)
  })
})

test.describe('malformed identifiers', () => {
  test('malformed values render as plain text without links', async ({ page }) => {
    const malformed = fullResourceArns()
    malformed.appEventBusArn = 'not-an-arn'
    malformed.integrationDlqArn = 'also-not-valid'
    malformed.platformEventBusArn = 'totally-bogus'

    await installDomainApi(page, domainDetail({
      resourceArns: { dev: malformed },
    }))
    await page.goto('/domains/orders')

    const panel = await envPanel(page)
    await expect(panel.getByText('not-an-arn')).toBeVisible()
    await expect(panel.getByText('also-not-valid')).toBeVisible()
    await expect(panel.getByText('totally-bogus')).toBeVisible()
    // No links should exist for malformed values
    const allLinks = panel.locator('a')
    for (const link of await allLinks.all()) {
      const text = await link.textContent()
      expect(text).not.toContain('not-an-arn')
      expect(text).not.toContain('also-not-valid')
      expect(text).not.toContain('totally-bogus')
    }
  })
})

test.describe('unknown identifiers', () => {
  test('unrecognized ARN services render as plain text', async ({ page }) => {
    const unknown = fullResourceArns()
    unknown.appEventBusArn = arn('lambda', 'function/acme-prod-handler')

    await installDomainApi(page, domainDetail({
      resourceArns: { dev: unknown },
    }))
    await page.goto('/domains/orders')

    const panel = await envPanel(page)
    await expect(panel.getByText(/lambda:.*acme-prod-handler/)).toBeVisible()
    const domainGroup = panel.locator('[data-resource-group="domain"]')
    // The lambda ARN is in a link-eligible field but not a recognized resource type
    const lambdaLink = domainGroup.locator('a').filter({ hasText: 'acme-prod-handler' })
    await expect(lambdaLink).toHaveCount(0)
  })
})

test.describe('missing values', () => {
  test('absent fields do not render their labels', async ({ page }) => {
    const sparse: Record<string, string | undefined> = {
      appEventBusArn: arn('events', 'event-bus/acme-prod-orders'),
    }

    await installDomainApi(page, domainDetail({
      resourceArns: { dev: sparse },
    }))
    await page.goto('/domains/orders')

    const panel = await envPanel(page)
    await expect(panel.getByText('appEventBusArn')).toBeVisible()
    await expect(panel.getByText('platformEventBusArn')).toHaveCount(0)
    await expect(panel.getByText('schemaRegistryArn')).toHaveCount(0)
    await expect(panel.getByText('subscriptionDlqArn')).toHaveCount(0)
    await expect(panel.getByText('integrationDlqArn')).toHaveCount(0)
    await expect(panel.getByText('dashboardArn')).toHaveCount(0)
  })

  test('empty resourceArns renders without errors', async ({ page }) => {
    await installDomainApi(page, domainDetail({
      resourceArns: {},
    }))
    await page.goto('/domains/orders')

    await expect(page.getByRole('heading', { name: 'Environments' })).toBeVisible()
    await expect(page.getByRole('button', { name: 'dev', exact: true })).toBeVisible()
  })
})

test.describe('account mismatch', () => {
  test('domain-account fields with wrong account are not linked', async ({ page }) => {
    const wrongAccount = fullResourceArns()
    wrongAccount.appEventBusArn = arn('events', 'event-bus/acme-prod-orders', '555566667777')
    wrongAccount.integrationDlqArn = `https://sqs.${REGION}.amazonaws.com/555566667777/acme-prod-integration-dlq`

    await installDomainApi(page, domainDetail({
      resourceArns: { dev: wrongAccount },
    }))
    await page.goto('/domains/orders')

    const panel = await envPanel(page)
    const domainGroup = panel.locator('[data-resource-group="domain"]')
    // Both identifiers are visible but not linked
    await expect(domainGroup.getByText(/acme-prod-orders/)).toBeVisible()
    await expect(domainGroup.getByText(/acme-prod-integration-dlq/)).toBeVisible()
    await expect(domainGroup.locator('a')).toHaveCount(0)
  })
})

test.describe('queue URLs', () => {
  test('integrationDlqArn as queue URL gets an SQS console link', async ({ page }) => {
    await installDomainApi(page, domainDetail())
    await page.goto('/domains/orders')

    const panel = await envPanel(page)
    const domainGroup = panel.locator('[data-resource-group="domain"]')
    const dlqLink = domainGroup.locator('a').filter({ hasText: 'acme-prod-integration-dlq' })
    await expect(dlqLink).toBeVisible()
    const href = await dlqLink.getAttribute('href')
    expect(href).toContain('console.aws.amazon.com/sqs')
    expect(href).toContain(REGION)
  })

  test('integrationDlqArn as ARN gets an SQS console link', async ({ page }) => {
    const withArnDlq = fullResourceArns()
    withArnDlq.integrationDlqArn = arn('sqs', 'acme-prod-integration-dlq')

    await installDomainApi(page, domainDetail({
      resourceArns: { dev: withArnDlq },
    }))
    await page.goto('/domains/orders')

    const panel = await envPanel(page)
    const domainGroup = panel.locator('[data-resource-group="domain"]')
    const dlqLink = domainGroup.locator('a').filter({ hasText: 'acme-prod-integration-dlq' })
    await expect(dlqLink).toBeVisible()
  })
})

test.describe('no ownerAccountId', () => {
  test('domain-account fields render as plain text when owner is unknown', async ({ page }) => {
    await installDomainApi(page, domainDetail({
      ownerAccountId: undefined,
    }))
    await page.goto('/domains/orders')

    const panel = await envPanel(page)
    const domainGroup = panel.locator('[data-resource-group="domain"]')
    await expect(domainGroup.getByText(/acme-prod-orders/)).toBeVisible()
    await expect(domainGroup.locator('a')).toHaveCount(0)
  })
})
