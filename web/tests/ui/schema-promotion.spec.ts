import { expect, test, type Page } from '@playwright/test'

import { SCHEMAS, freshState, installApi, signIn } from './api'

/**
 * Producer schema page (`/domains/{domain}/schemas/{schema}`): the promotion
 * pipeline in configured environment order, per-version environment reach,
 * subscriber pins, and the promote journey (confirm, progress polling,
 * conflict and failure explanations).
 */

const PAGE = '/domains/orders/schemas/acme-orders%40OrderNotification'

function card(page: Page, title: string) {
  return page.locator('section').filter({
    has: page.getByRole('heading', { name: title, exact: true }),
  })
}

function seedPipeline() {
  // dev (authoring) is ahead of both targets; the fixture's insertion order
  // intentionally disagrees with the configured order.
  SCHEMAS[0]!.latestVersions = { prod: 3, dev: 5, staging: 3 }
}

test.beforeEach(async ({ page }) => {
  seedPipeline()
  await signIn(page, ['platform-admin'])
})

test('pipeline nodes follow the configured order with behind counts and not-yet-promoted nodes', async ({ page }) => {
  SCHEMAS[0]!.latestVersions = { prod: 3, dev: 5 }
  await installApi(page)
  await page.goto(PAGE)

  const pipeline = card(page, 'Promotion pipeline')
  const nodes = pipeline.getByRole('listitem')
  await expect(nodes).toHaveCount(3)
  await expect(nodes.nth(0)).toContainText('dev')
  await expect(nodes.nth(0)).toContainText('authoring')
  await expect(nodes.nth(0)).toContainText('v5')
  await expect(nodes.nth(1)).toContainText('staging')
  await expect(nodes.nth(1)).toContainText('not promoted yet')
  await expect(nodes.nth(1)).toContainText('5 behind')
  await expect(nodes.nth(2)).toContainText('prod')
  await expect(nodes.nth(2)).toContainText('v3')
  await expect(nodes.nth(2)).toContainText('2 behind')
})

test('only the environment whose predecessor holds new versions offers the promote button', async ({ page }) => {
  // dev v5, staging v3, prod v3: prod's predecessor (staging) holds nothing
  // prod lacks, so only staging is promotable.
  await installApi(page)
  await page.goto(PAGE)

  const pipeline = card(page, 'Promotion pipeline')
  await expect(pipeline.getByRole('button', { name: 'Promote to staging' })).toBeVisible()
  await expect(pipeline.getByRole('button', { name: 'Promote to prod' })).toHaveCount(0)
})

test('a caught-up middle environment moves the affordance to the next gap', async ({ page }) => {
  SCHEMAS[0]!.latestVersions = { prod: 3, dev: 5, staging: 5 }
  await installApi(page)
  await page.goto(PAGE)

  const pipeline = card(page, 'Promotion pipeline')
  await expect(pipeline.getByRole('button', { name: 'Promote to staging' })).toHaveCount(0)
  await expect(pipeline.getByRole('button', { name: 'Promote to prod' })).toBeVisible()
})

test('domain page badges follow the configured order and link to the producer schema page', async ({ page }) => {
  await installApi(page)
  await page.goto('/domains/orders')

  const row = page.getByRole('listitem').filter({ hasText: 'acme-orders@OrderNotification' })
  const badges = row.getByText(/^(dev|staging|prod) v\d$/)
  await expect(badges.first()).toHaveText('dev v5', { exact: true })
  await expect(badges.nth(1)).toHaveText('staging v3', { exact: true })
  await expect(badges.nth(2)).toHaveText('prod v3', { exact: true })

  // Registration is offered from the panel regardless of list state; per-schema
  // version management lives on each producer schema page.
  await expect(page.getByRole('button', { name: 'New version' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Register schema' })).toBeVisible()

  await row.getByRole('link', { name: 'acme-orders@OrderNotification' }).click()
  await expect(page).toHaveURL(/\/domains\/orders\/schemas\/acme-orders/)
  await expect(card(page, 'Promotion pipeline')).toBeVisible()
})

test('registering another schema from the panel opens the wide authoring modal', async ({ page }) => {
  await installApi(page)
  await page.goto('/domains/orders')

  await page.getByRole('button', { name: 'Register schema' }).click()

  const dialog = page.getByRole('dialog')
  await expect(dialog).toBeVisible()
  await expect(dialog.getByRole('heading', { name: 'Register schema' })).toBeVisible()
  await expect(dialog.getByLabel('Event name')).toBeVisible()
  await expect(dialog.getByText('Target environment: dev')).toBeVisible()

  // The authoring modal is the wide variant (max-w-2xl, not the max-w-lg default).
  await expect(dialog).toHaveClass(/nx-dialog--lg/)

  await dialog.getByRole('button', { name: 'Cancel' }).click()
  await expect(dialog).toHaveCount(0)
})

/** One producer schema per name, modified on consecutive October days:
 * Alpha on the 24th … each later name one day later. */
function schemasFixture(names: string[]) {
  return names.map((name, index) => ({
    schemaName: `acme-orders@${name}`,
    producerDomain: 'orders',
    source: 'acme-orders',
    detailType: name,
    latestVersions: { dev: 1 },
    metadataRevisions: {},
    lastModifiedAt: `2026-10-${String(24 + index).padStart(2, '0')}T09:00:00+00:00`,
  }))
}

function fulfillJson(route: import('@playwright/test').Route, body: unknown) {
  return route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) })
}

test('the domain panel caps at the five most recently modified schemas and links to the full view', async ({ page }) => {
  await installApi(page)
  await page.route('**/api/dev/schemas**', route =>
    fulfillJson(route, { items: schemasFixture(['Alpha', 'Bravo', 'Charlie', 'Delta', 'Echo', 'Foxtrot', 'Golf']) }))
  await page.goto('/domains/orders')

  const panel = card(page, 'Schemas')
  const rows = panel.getByRole('listitem')
  await expect(rows).toHaveCount(5)
  // Newest modification first: Golf (10-30) down to Charlie (10-26).
  await expect(panel.getByRole('link', { name: 'acme-orders@Golf' })).toBeVisible()
  await expect(panel.getByRole('link', { name: 'acme-orders@Charlie' })).toBeVisible()
  await expect(panel.getByRole('link', { name: 'acme-orders@Bravo' })).toHaveCount(0)
  await expect(panel.getByRole('link', { name: 'acme-orders@Alpha' })).toHaveCount(0)
  await expect(rows.first().getByText('Modified 2026-10-30', { exact: true })).toBeVisible()

  const viewAll = panel.getByRole('link', { name: 'View all schemas' })
  await expect(viewAll).toBeVisible()
  await expect(viewAll).toHaveAttribute('href', '/domains/orders/schemas')
})

test('the domain panel shows every schema when five or fewer exist', async ({ page }) => {
  await installApi(page)
  await page.route('**/api/dev/schemas**', route =>
    fulfillJson(route, { items: schemasFixture(['Alpha', 'Bravo', 'Charlie']) }))
  await page.goto('/domains/orders')

  const panel = card(page, 'Schemas')
  await expect(panel.getByRole('listitem')).toHaveCount(3)
  await expect(panel.getByRole('link', { name: 'View all schemas' })).toHaveCount(0)
})

test('the domain panel ignores an explicit null continuation token', async ({ page }) => {
  await installApi(page)
  // The deployed catalog returns `"nextToken": null` on its last page; the
  // panel must not treat that as "more pages exist".
  await page.route('**/api/dev/schemas**', route =>
    fulfillJson(route, { items: schemasFixture(['Alpha', 'Bravo']), nextToken: null }))
  await page.goto('/domains/orders')

  const panel = card(page, 'Schemas')
  await expect(panel.getByRole('listitem')).toHaveCount(2)
  await expect(panel.getByRole('link', { name: 'View all schemas' })).toHaveCount(0)
})

test('the dedicated schemas page lists every schema across pages', async ({ page }) => {
  await installApi(page)
  const all = schemasFixture(['Alpha', 'Bravo', 'Charlie', 'Delta', 'Echo', 'Foxtrot', 'Golf'])
  await page.route('**/api/dev/schemas**', (route) => {
    const token = new URL(route.request().url()).searchParams.get('nextToken')
    return token
      ? fulfillJson(route, { items: all.slice(3) })
      : fulfillJson(route, { items: all.slice(0, 3), nextToken: 'page-2' })
  })
  await page.goto('/domains/orders/schemas')

  await expect(page.getByRole('heading', { name: 'Schemas', exact: true })).toBeVisible()
  await expect(page.getByRole('listitem')).toHaveCount(3)
  await expect(page.getByRole('link', { name: 'acme-orders@Delta' })).toHaveCount(0)

  await page.getByRole('button', { name: 'Load more' }).click()
  await expect(page.getByRole('listitem')).toHaveCount(7)
  await expect(page.getByRole('link', { name: 'acme-orders@Golf' })).toBeVisible()
  await expect(page.getByRole('listitem').last().getByText('Modified 2026-10-30', { exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Load more' })).toHaveCount(0)

  await page.getByRole('link', { name: 'acme-orders@Golf' }).click()
  await expect(page).toHaveURL(/\/domains\/orders\/schemas\/acme-orders%40Golf/)
})

test('version history lists the environments each version has reached', async ({ page }) => {
  await installApi(page)
  await page.goto(PAGE)

  const history = card(page, 'Version history')
  const rows = history.getByRole('row')
  await expect(rows).toHaveCount(6)
  const v5 = history.getByRole('row').filter({ hasText: 'v5' })
  await expect(v5.getByText('dev', { exact: true })).toBeVisible()
  await expect(v5.getByText('staging', { exact: true })).toBeVisible()
  await expect(v5.getByText('prod', { exact: true })).toBeVisible()
  const v3 = history.getByRole('row').filter({ hasText: 'v3' })
  await expect(v3.getByText('everywhere', { exact: true })).toBeVisible()
})

test('subscribers list shows pinned versions and paused environments', async ({ page }) => {
  await installApi(page)
  await page.goto(PAGE)

  const subscribers = card(page, 'Subscribers')
  const billing = subscribers.getByRole('row').filter({ hasText: 'billing' })
  await expect(billing.getByText('dev v2', { exact: true })).toBeVisible()
  await expect(billing.getByText('staging v3', { exact: true })).toBeVisible()
  await expect(billing.getByText('prod v3', { exact: true })).toBeVisible()
  const loyalty = subscribers.getByRole('row').filter({ hasText: 'loyalty' })
  await expect(loyalty.getByText('prod v1 paused', { exact: true })).toBeVisible()
})

test('promote journey: confirm names the replay and re-pin, progress polls to completion', async ({ page }) => {
  await installApi(page)
  await page.goto(PAGE)

  await card(page, 'Promotion pipeline').getByRole('button', { name: 'Promote to staging' }).click()
  const dialog = page.getByRole('alertdialog')
  await expect(dialog.getByText(/replays versions v4–v5 from dev into staging/)).toBeVisible()
  await expect(dialog.getByText('1 active subscription in staging will be re-pinned')).toBeVisible()
  await dialog.getByRole('button', { name: 'Promote', exact: true }).click()

  const promotions = card(page, 'Promotions')
  await expect(promotions.getByText('dev → staging')).toBeVisible()
  await expect(promotions.getByText('Replaying versions')).toBeVisible({ timeout: 10_000 })
  await expect(
    page.getByText('OrderNotification is now at v5 in staging', { exact: true }),
  ).toBeVisible({ timeout: 20_000 })
  await expect(promotions.getByText(/Now at v5 in staging, 2 of 2 subscriptions re-pinned/)).toBeVisible()
})

test('a conflicting promote reports the in-flight promotion and shows its progress', async ({ page }) => {
  // The race the 409 contract exists for: the page loads clean, then another
  // producer starts a promotion before this one confirms.
  const state = freshState()
  await installApi(page, state)
  await page.goto(PAGE)
  await expect(card(page, 'Promotion pipeline')).toBeVisible()

  state.promotionMode = 'conflict'
  state.promotions['staging::OrderNotification'] = {
    promotionId: 'promo-existing',
    schemaName: 'OrderNotification',
    sourceEnvironment: 'dev',
    targetEnvironment: 'staging',
    requestedVersion: 5,
    status: 'REPLAYING',
    replayedVersions: [4],
    createdAt: '2026-10-01T09:00:00.000Z',
    updatedAt: '2026-10-01T09:00:00.000Z',
  }

  await card(page, 'Promotion pipeline').getByRole('button', { name: 'Promote to staging' }).click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'Promote', exact: true }).click()

  await expect(
    page.getByText('A promotion is already in progress for staging — its progress is shown below.', { exact: true }),
  ).toBeVisible()
  await expect(card(page, 'Promotions').getByText('REPLAYING', { exact: true })).toBeVisible()
})

test('in-flight progress survives leaving and returning to the page', async ({ page }) => {
  const state = freshState()
  state.promotionMode = 'hold'
  state.promotions['staging::OrderNotification'] = {
    promotionId: 'promo-running',
    schemaName: 'OrderNotification',
    sourceEnvironment: 'dev',
    targetEnvironment: 'staging',
    requestedVersion: 5,
    status: 'REPLAYING',
    replayedVersions: [4],
    createdAt: '2026-10-01T09:00:00.000Z',
    updatedAt: '2026-10-01T09:00:00.000Z',
  }
  await installApi(page, state)
  await page.goto(PAGE)

  await expect(card(page, 'Promotions').getByText('REPLAYING', { exact: true })).toBeVisible()
  await page.goto('/domains/orders')
  await expect(card(page, 'Schemas')).toBeVisible()
  await page.goto(PAGE)
  await expect(card(page, 'Promotions').getByText('REPLAYING', { exact: true })).toBeVisible()
  await expect(card(page, 'Promotions').getByText('Replaying versions — 1 written so far.')).toBeVisible()
})

test('a diverged history explains the stop and keeps replayed versions in place', async ({ page }) => {
  const state = freshState()
  state.promotionMode = 'fail-diverged'
  await installApi(page, state)
  await page.goto(PAGE)

  await card(page, 'Promotion pipeline').getByRole('button', { name: 'Promote to staging' }).click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'Promote', exact: true }).click()

  const promotions = card(page, 'Promotions')
  await expect(promotions.getByText('FAILED', { exact: true })).toBeVisible({ timeout: 15_000 })
  await expect(promotions.getByText(/diverges from dev/)).toBeVisible()
  await expect(promotions.getByText(/No versions were written/)).toBeVisible()
})

test('a skipped-environment refusal explains the pipeline order', async ({ page }) => {
  const state = freshState()
  state.promotionMode = 'fail-skipped'
  await installApi(page, state)
  await page.goto(PAGE)

  await card(page, 'Promotion pipeline').getByRole('button', { name: 'Promote to staging' }).click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'Promote', exact: true }).click()

  const promotions = card(page, 'Promotions')
  await expect(promotions.getByText('FAILED', { exact: true })).toBeVisible({ timeout: 15_000 })
  await expect(promotions.getByText(/can only receive versions promoted from the environment immediately before it/)).toBeVisible()
})

test('a replay failure names the requested version and says replay skips what is already written', async ({ page }) => {
  const state = freshState()
  state.promotionMode = 'fail-replay'
  await installApi(page, state)
  await page.goto(PAGE)

  await card(page, 'Promotion pipeline').getByRole('button', { name: 'Promote to staging' }).click()
  await page.getByRole('alertdialog').getByRole('button', { name: 'Promote', exact: true }).click()

  const promotions = card(page, 'Promotions')
  await expect(promotions.getByText('FAILED', { exact: true })).toBeVisible({ timeout: 15_000 })
  await expect(promotions.getByText(/Replay failed before reaching v5/)).toBeVisible()
  await expect(promotions.getByText(/staging is now at v4/)).toBeVisible()
  await expect(promotions.getByText(/already-replayed versions are skipped/)).toBeVisible()
})

test('a caller without producer access sees the access-required state and no pipeline data', async ({ page }) => {
  await signIn(page, ['team-billing'])
  await installApi(page)
  await page.route('**/api/*/schemas/*OrderNotification**', async (route) => {
    await route.fulfill({
      status: 403,
      contentType: 'application/json',
      body: JSON.stringify({ code: 'AccessDenied', message: 'publisher binding required' }),
    })
  })
  await page.goto(PAGE)

  await expect(page.getByText('Publisher access required')).toBeVisible()
  await expect(card(page, 'Promotion pipeline')).toHaveCount(0)
  await expect(card(page, 'Version history')).toHaveCount(0)
  await expect(page.getByRole('button', { name: /Promote to/ })).toHaveCount(0)
})

test('an auditor reads the pipeline and history without any promote affordance', async ({ page }) => {
  await signIn(page, ['platform-auditor'])
  await installApi(page)
  await page.goto(PAGE)

  await expect(card(page, 'Promotion pipeline').getByText('authoring')).toBeVisible()
  await expect(card(page, 'Version history')).toBeVisible()
  await expect(page.getByRole('button', { name: /Promote to/ })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'New version' })).toHaveCount(0)
})

test('a publisher inspects an environment\'s content from the pipeline', async ({ page }) => {
  await installApi(page)
  await page.goto(PAGE)

  await card(page, 'Promotion pipeline')
    .getByRole('button', { name: 'View schema content in staging' })
    .click()

  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('heading', { name: 'staging · v3' })).toBeVisible()
  await expect(dialog.locator('pre')).toContainText('"description": "OrderNotification v3"')
  await expect(dialog.locator('pre')).toContainText('"orderId"')
})

test('the viewer copies the shown content to the clipboard', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write'])
  await installApi(page)
  await page.goto(PAGE)

  await card(page, 'Promotion pipeline')
    .getByRole('button', { name: 'View schema content in dev' })
    .click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('heading', { name: 'dev · v5' })).toBeVisible()

  const copyButton = dialog.getByRole('button', { name: 'Copy schema content JSON' })
  await copyButton.click()
  await expect(copyButton).toHaveText('Copied')
  const clipboard = await page.evaluate(() => navigator.clipboard.readText())
  expect(JSON.parse(clipboard).description).toBe('OrderNotification v5')
})

test('version history inspects the version that reached each environment', async ({ page }) => {
  await installApi(page)
  await page.goto(PAGE)

  // dev runs v5, but the v3 history row must serve v3's content, not the latest.
  const v3 = card(page, 'Version history').getByRole('row').filter({ hasText: 'v3' })
  await v3.getByRole('button', { name: 'View v3 content in dev' }).click()

  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('heading', { name: 'dev · v3', exact: true })).toBeVisible()
  await expect(dialog.locator('pre')).toContainText('"description": "OrderNotification v3"')
})

test('an environment without the schema offers no view affordance', async ({ page }) => {
  SCHEMAS[0]!.latestVersions = { prod: 3, dev: 5 }
  await installApi(page)
  await page.goto(PAGE)

  const stagingNode = card(page, 'Promotion pipeline')
    .getByRole('listitem')
    .filter({ hasText: 'not promoted yet' })
  await expect(stagingNode.getByRole('button', { name: 'View schema content in staging' })).toHaveCount(0)
})

test('an auditor inspects content read-only', async ({ page }) => {
  await signIn(page, ['platform-auditor'])
  await installApi(page)
  await page.goto(PAGE)

  await card(page, 'Promotion pipeline')
    .getByRole('button', { name: 'View schema content in prod' })
    .click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('heading', { name: 'prod · v3' })).toBeVisible()
  await expect(dialog.locator('pre')).toContainText('"orderId"')
  await expect(page.getByRole('button', { name: /Promote to/ })).toHaveCount(0)
})

test('a content lookup failure surfaces as a toast and the dialog stays dismissible', async ({ page }) => {
  const state = freshState()
  state.failContent = true
  await installApi(page, state)
  await page.goto(PAGE)

  await card(page, 'Promotion pipeline')
    .getByRole('button', { name: 'View schema content in dev' })
    .click()

  await expect(
    page.locator('[data-state="open"]').filter({ hasText: 'schema lookup failed' }),
  ).toBeVisible()

  const dialog = page.getByRole('dialog')
  await expect(dialog.getByRole('alert')).toHaveText('The content could not be loaded.')
  await dialog.getByRole('button', { name: 'Close' }).last().click()
  await expect(page.getByRole('dialog')).toHaveCount(0)
})
