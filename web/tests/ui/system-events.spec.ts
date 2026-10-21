import { expect, test } from './fixtures'

import { installApi, signIn } from './api'

/**
 * Top-level System events page and the header nav indicator.
 *
 * The page lists every visible domain's platform-managed SNS topic ARN once,
 * copyable with inline confirmation, plus the newest-event marker the router
 * records. The nav item carries a dedicated icon with a browser-local unread
 * dot (localStorage last-seen vs. the newest marker); there is no event feed
 * and no server-side read state. The page never offers topic administration,
 * subscription management, console, or replay controls.
 */

const ORDERS_ARN = 'arn:aws:sns:us-east-1:111122223333:acme-orders-system-events'
const BILLING_ARN = 'arn:aws:sns:us-east-1:444455556666:acme-billing-system-events'
const SEEN_KEY = 'konductor:system-events:last-seen'
const MARKER = '2026-10-21T09:15:00.000Z'

function summary(name: string, overrides: Record<string, unknown> = {}) {
  return {
    domainName: name,
    status: 'ACTIVE',
    description: `${name} events`,
    ownerEmail: `${name}@example.com`,
    createdAt: '2026-08-01T09:00:00.000Z',
    ...overrides,
  }
}

function detail(name: string, overrides: Record<string, unknown> = {}) {
  return {
    ...summary(name),
    ownerAccountId: '111122223333',
    environments: ['dev'],
    ...overrides,
  }
}

function fulfillJson(route: import('@playwright/test').Route, body: unknown) {
  return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })
}

interface InstallOptions {
  summaries: Record<string, unknown>[]
  details: Record<string, Record<string, unknown>>
}

/** Override the harness list/describe routes (registered later, so they win). */
async function installSystemEventsApi(page: import('@playwright/test').Page, options: InstallOptions) {
  await page.route('**/api/dev/domains', async (route) => {
    const url = new URL(route.request().url())
    if (url.searchParams.get('view') === 'directory') {
      return fulfillJson(route, {})
    }
    return fulfillJson(route, { domains: options.summaries })
  })
  for (const [name, body] of Object.entries(options.details)) {
    await page.route(`**/api/dev/domains/${name}`, (route) => fulfillJson(route, body))
  }
}

async function seedSeen(page: import('@playwright/test').Page, value: string | null) {
  await page.addInitScript(
    ([key, stored]) => {
      if (stored === null) {
        window.localStorage.removeItem(key as string)
      } else {
        window.localStorage.setItem(key as string, stored as string)
      }
    },
    [SEEN_KEY, value],
  )
}

function navLink(page: import('@playwright/test').Page) {
  return page.getByRole('link', { name: /System events/ })
}

test.beforeEach(async ({ page }) => {
  await signIn(page, ['platform-admin'])
  await installApi(page)
})

test('top-level nav carries System events with its own icon', async ({ page }) => {
  await page.goto('/')

  const link = navLink(page)
  await expect(link).toBeVisible()
  await expect(link.locator('svg')).toHaveCount(1)
})

test('unread dot appears when a delivered event is newer than the last visit', async ({ page }) => {
  await seedSeen(page, '2026-10-20T08:00:00.000Z')
  await installSystemEventsApi(page, {
    summaries: [summary('orders', { systemEventsLastEventAt: MARKER, systemEventsLastEventType: 'SubscriptionUpgraded' })],
    details: {},
  })
  await page.goto('/')

  await expect(page.getByTestId('system-events-unread-dot')).toBeVisible()
})

test('unread dot stays hidden when the stored visit covers the newest event', async ({ page }) => {
  await seedSeen(page, MARKER)
  await installSystemEventsApi(page, {
    summaries: [summary('orders', { systemEventsLastEventAt: MARKER })],
    details: {},
  })
  await page.goto('/')

  await expect(page.getByTestId('system-events-unread-dot')).toHaveCount(0)
})

test('visiting the page records the newest marker and clears the dot', async ({ page }) => {
  await seedSeen(page, null)
  await installSystemEventsApi(page, {
    summaries: [summary('orders', { systemEventsLastEventAt: MARKER })],
    details: { orders: detail('orders', { systemEventsTopicArn: ORDERS_ARN, systemEventsLastEventAt: MARKER }) },
  })
  await page.goto('/')

  await expect(page.getByTestId('system-events-unread-dot')).toBeVisible()
  await page.goto('/system-events')
  await expect(page.getByRole('heading', { name: 'System events' })).toBeVisible()

  // Read state is browser-local: localStorage, never a server call
  const stored = await page.evaluate((key) => window.localStorage.getItem(key), SEEN_KEY)
  expect(stored).toBe(MARKER)
  await expect(page.getByTestId('system-events-unread-dot')).toHaveCount(0)

  await page.goto('/domains')
  await expect(page.getByTestId('system-events-unread-dot')).toHaveCount(0)
})

test('page lists each domain topic once with copy and the last-event marker', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  await installSystemEventsApi(page, {
    summaries: [
      summary('orders', { systemEventsLastEventAt: MARKER, systemEventsLastEventType: 'SubscriptionUpgraded' }),
      summary('billing'),
    ],
    details: {
      orders: detail('orders', {
        systemEventsTopicArn: ORDERS_ARN,
        systemEventsLastEventAt: MARKER,
        systemEventsLastEventType: 'SubscriptionUpgraded',
      }),
      billing: detail('billing', { systemEventsTopicArn: BILLING_ARN }),
    },
  })
  await page.goto('/system-events')

  const ordersCard = page.locator('section', { has: page.getByRole('heading', { name: 'orders', exact: true }) })
  await expect(ordersCard.getByText(ORDERS_ARN)).toBeVisible()
  // One ARN per card; never repeated elsewhere on the page
  await expect(page.getByText(ORDERS_ARN)).toHaveCount(1)
  await expect(page.getByText(BILLING_ARN)).toHaveCount(1)
  await expect(ordersCard.getByText(/Last system event: SubscriptionUpgraded/)).toBeVisible()

  await ordersCard.getByRole('button', { name: 'Copy system events topic ARN for orders' }).click()
  await expect(ordersCard.getByRole('button', { name: 'Copy system events topic ARN for orders' })).toHaveText('Copied')

  // The page explains filter policies and identifies topics as platform-managed
  await expect(page.getByText(/platform-managed SNS topic/)).toBeVisible()
  await expect(page.getByText(/filter policy/)).toBeVisible()
})

test('domains without a recorded topic show the provisioning guidance', async ({ page }) => {
  await installSystemEventsApi(page, {
    summaries: [summary('orders')],
    details: { orders: detail('orders') },
  })
  await page.goto('/system-events')

  const card = page.locator('section', { has: page.getByRole('heading', { name: 'orders', exact: true }) })
  await expect(card.getByText(/after platform provisioning or the topic backfill runs/)).toBeVisible()
  await expect(card.getByRole('button', { name: /Copy system events topic ARN/ })).toHaveCount(0)
})

test('the page never offers topic administration or replay controls', async ({ page }) => {
  await installSystemEventsApi(page, {
    summaries: [summary('orders')],
    details: { orders: detail('orders', { systemEventsTopicArn: ORDERS_ARN }) },
  })
  await page.goto('/system-events')

  await expect(page.getByRole('button', { name: /subscribe|unsubscribe|delete topic|replay/i })).toHaveCount(0)
  await expect(page.locator('a[href*="console.aws.amazon.com/sns"]')).toHaveCount(0)
})

test('auditors see the same page as operators', async ({ page }) => {
  await signIn(page, ['platform-auditor'], 'auditor@example.com')
  await installSystemEventsApi(page, {
    summaries: [summary('orders')],
    details: { orders: detail('orders', { systemEventsTopicArn: ORDERS_ARN }) },
  })
  await page.goto('/system-events')

  await expect(page.getByRole('heading', { name: 'System events' })).toBeVisible()
  await expect(page.getByText(ORDERS_ARN)).toBeVisible()
})

test('long ARNs stay wrapped and operable at 375px', async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 667 })
  await installSystemEventsApi(page, {
    summaries: [summary('orders')],
    details: { orders: detail('orders', { systemEventsTopicArn: ORDERS_ARN }) },
  })
  await page.goto('/system-events')

  const arn = page.getByText(ORDERS_ARN)
  await expect(arn).toBeVisible()
  // The ARN wraps inside the card instead of extending past the viewport
  const arnBox = await arn.boundingBox()
  expect(arnBox).not.toBeNull()
  expect(arnBox!.x).toBeGreaterThanOrEqual(0)
  expect(arnBox!.x + arnBox!.width).toBeLessThanOrEqual(375)

  const copy = page.getByRole('button', { name: 'Copy system events topic ARN for orders' })
  await copy.focus()
  await expect(copy).toBeFocused()

  // Recorded gap (see viewport.spec.ts): the app header itself overflows
  // horizontally at 375px; this page does not add to it.
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  )
  test.info().annotations.push({ type: 'note', description: `pre-existing header overflow at 375px: ${overflow}px` })
})

test('empty state explains visibility when no domains are listed', async ({ page }) => {
  await installSystemEventsApi(page, { summaries: [], details: {} })
  await page.goto('/system-events')

  await expect(page.getByRole('heading', { name: 'No domains' })).toBeVisible()
  await expect(
    page.getByText('No domains are visible to you. Register a domain or ask a platform administrator about access.'),
  ).toBeVisible()
})

test('a failed domain detail fetch is not shown as a missing topic', async ({ page }) => {
  await installSystemEventsApi(page, {
    summaries: [summary('orders')],
    details: {},
  })
  await page.route('**/api/dev/domains/orders', (route) =>
    route.fulfill({ status: 500, contentType: 'application/json', body: JSON.stringify({ message: 'boom' }) }),
  )
  await page.goto('/system-events')

  const card = page.locator('section', { has: page.getByRole('heading', { name: 'orders', exact: true }) })
  await expect(card.getByText(/Couldn't load this domain's details/)).toBeVisible()
  await expect(card.getByText(/No system-events topic is configured/)).toHaveCount(0)
})

test('the domain detail page no longer embeds a System events card', async ({ page }) => {  await page.route('**/api/dev/domains/orders', (route) =>
    fulfillJson(route, detail('orders', { systemEventsTopicArn: ORDERS_ARN })),
  )
  await page.goto('/domains/orders')

  await expect(page.getByRole('heading', { name: 'System events' })).toHaveCount(0)
  await expect(page.getByText(ORDERS_ARN)).toHaveCount(0)
})
