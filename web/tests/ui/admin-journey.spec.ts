import { expect, test, type Page } from '@playwright/test'

import { installApi, signIn } from './api'

/**
 * The full metadata administration journey as one continuous story:
 * create (draft extension) → preview → save → conflict recovery →
 * activate (readiness + inventory + explicit confirmation) → inspect.
 * Mock state carries across steps inside each test, so the journey reads
 * like an operator's session instead of isolated fragments.
 */

/** One NxCard, addressed by its heading (cards repeat labels across panels). */
function card(page: Page, title: string) {
  return page.locator('section').filter({
    has: page.getByRole('heading', { name: title, exact: true }),
  })
}

/** Root-level editor controls render after the nested per-object ones, so the root Add field button is the last match. */
function addedField(page: Page) {
  return page.getByRole('textbox', { name: 'Field name', exact: true }).last()
}

test.beforeEach(async ({ page }) => {
  await signIn(page, ['platform-admin'])
})

test('loads the configuration surface with defaults, environments, and history', async ({ page }) => {
  await installApi(page)
  await page.goto('/admin/metadata')

  await expect(page.getByRole('heading', { name: 'Metadata administration' })).toBeVisible()
  await expect(page.getByText('Read and write')).toBeVisible()
  await expect(card(page, 'Protected defaults').getByText('idempotencyKey')).toBeVisible()

  await expect(card(page, 'Environments').getByRole('cell', { name: 'prod', exact: true })).toBeVisible()
  await expect(card(page, 'Environments').getByText('Legacy', { exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Prepare activation' }).first()).toBeVisible()
  await expect(page.getByRole('button', { name: 'Inspect revision M2' })).toBeVisible()
})

test('locked inherited fields stay immutable while additions remain editable', async ({ page }) => {
  await installApi(page)
  await page.goto('/admin/metadata')

  const inheritedName = page.getByRole('textbox', { name: 'Field name', exact: true }).first()
  await expect(inheritedName).toBeDisabled()
  await expect(page.getByText('Immutable', { exact: true }).first()).toBeVisible()
  await expect(page.getByRole('button', { name: 'Remove field eventId' })).toHaveCount(0)

  await page.getByRole('button', { name: 'Add field', exact: true }).last().click()
  await expect(addedField(page)).toBeEnabled()
})

test('violations label the offending control and retain the typed value', async ({ page }) => {
  await installApi(page)
  await page.goto('/admin/metadata')

  await page.getByRole('button', { name: 'Add field', exact: true }).last().click()
  const added = addedField(page)
  await added.fill('schemaVersion')
  await expect(page.getByText('reserved built-in metadata field cannot be redefined')).toBeVisible()
  await expect(added).toHaveValue('schemaVersion')

  await added.fill('')
  await expect(page.getByText('field name must match [A-Za-z][A-Za-z0-9_]{0,63} (1-64 characters, starting with a letter)')).toBeVisible()
})

test('preview shows the candidate revision, generated fields, and bounds without saving', async ({ page }) => {
  await installApi(page)
  await page.goto('/admin/metadata')

  await page.getByRole('button', { name: 'Add field', exact: true }).last().click()
  await addedField(page).fill('traceId')
  await page.getByRole('button', { name: 'Preview draft' }).click()

  const preview = card(page, 'Draft preview')
  await expect(preview.getByText('The draft would become revision M3')).toBeVisible()
  await expect(preview.getByText('uuid-v4').first()).toBeVisible()
  await expect(preview.getByText(/Bounds: \d+\/64 extension nodes/)).toBeVisible()
  await expect(preview.getByText('Not saved, not published, not activated')).toBeVisible()
})

test('save publishes M3 to the history and resets the draft', async ({ page }) => {
  await installApi(page)
  await page.goto('/admin/metadata')

  await page.getByRole('button', { name: 'Add field', exact: true }).last().click()
  await addedField(page).fill('traceId')
  await page.getByRole('button', { name: 'Save new revision' }).click()

  await expect(page.getByText('Revision M3 saved; it is not activated', { exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Inspect revision M3' })).toBeVisible()
  await expect(page.getByText('Draft matches the current head')).toBeVisible()
})

test('concurrent save keeps local edits behind a conflict banner', async ({ page }) => {
  const state = await installApi(page)
  await page.goto('/admin/metadata')

  await page.getByRole('button', { name: 'Add field', exact: true }).last().click()
  await addedField(page).fill('traceId')
  await page.getByRole('button', { name: 'Save new revision' }).click()
  await expect(page.getByText('Revision M3 saved; it is not activated', { exact: true })).toBeVisible()

  await page.getByRole('button', { name: 'Add field', exact: true }).last().click()
  const second = addedField(page)
  await second.fill('correlationId')
  state.conflictNextSave = true
  await page.getByRole('button', { name: 'Save new revision' }).click()

  await expect(page.getByText('Another administrator saved a newer revision first')).toBeVisible()
  await expect(second).toHaveValue('correlationId')

  await page.getByRole('button', { name: 'Reload latest history' }).click()
  await expect(page.getByText('History reloaded; your draft is kept for review')).toBeVisible()
  await expect(second).toHaveValue('correlationId')
})

test('activation runs readiness, inventory, explicit acknowledgement, and audit', async ({ page }) => {
  await installApi(page)
  await page.goto('/admin/metadata')

  await page.getByRole('button', { name: 'Prepare activation for prod' }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByText('M0 (legacy baseline)')).toBeVisible()

  await expect(dialog.getByText('acme-orders@OrderNotification')).toBeVisible()
  await dialog.getByRole('button', { name: 'Load more schemas' }).click()
  await expect(dialog.getByText('acme-loyalty@MemberUpdated')).toBeVisible()

  await dialog.getByRole('button', { name: 'Check readiness' }).click()
  await expect(dialog.getByText('Definition recomplies exactly:')).toBeVisible()
  await expect(dialog.getByText(/Observed .+/).first()).toBeVisible()

  await dialog.getByRole('button', { name: 'Continue to activation' }).click()
  const confirm = page.getByRole('dialog').filter({ hasText: 'Activate M2 for prod' })
  await expect(confirm.getByText('Consumers must tolerate metadata additions')).toBeVisible()
  const activate = confirm.getByRole('button', { name: 'Activate M2 for prod' })
  await expect(activate).toBeDisabled()

  await confirm.getByRole('checkbox').check()
  await activate.click()

  await expect(page.getByText('prod is now active at M2', { exact: true })).toBeVisible()
  const prodRow = card(page, 'Environments').getByRole('row').filter({ hasText: 'prod' })
  await expect(prodRow.getByRole('cell', { name: '2', exact: true })).toBeVisible()
  await expect(prodRow.getByText('Legacy', { exact: true })).toHaveCount(0)
  await expect(card(page, 'Activation history').getByText('M0 → M2')).toBeVisible()
})

test('in-flight activation refuses Escape and click-away dismissal', async ({ page }) => {
  const state = await installApi(page)
  state.activationDelayMs = 1500
  await page.goto('/admin/metadata')

  await page.getByRole('button', { name: 'Prepare activation for prod' }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByRole('button', { name: 'Continue to activation' }).click()
  const confirm = page.getByRole('dialog').filter({ hasText: 'Activate M2 for prod' })
  await confirm.getByRole('checkbox').check()
  await confirm.getByRole('button', { name: 'Activate M2 for prod' }).click()

  await expect(confirm.getByText('Activating…')).toBeVisible()
  await page.keyboard.press('Escape')
  await expect(confirm).toBeVisible()

  await expect(confirm).toBeHidden({ timeout: 10_000 })
  await expect(page.getByText('prod is now active at M2', { exact: true })).toBeVisible()
})

test('stale activation closes the confirm, shows the banner, and refreshes identity', async ({ page }) => {
  const state = await installApi(page)
  state.activationMode = 'stale'
  await page.goto('/admin/metadata')

  await page.getByRole('button', { name: 'Prepare activation for prod' }).click()
  const dialog = page.getByRole('dialog')
  await dialog.getByRole('button', { name: 'Continue to activation' }).click()
  const confirm = page.getByRole('dialog').filter({ hasText: 'Activate M2 for prod' })
  await confirm.getByRole('checkbox').check()
  await confirm.getByRole('button', { name: 'Activate M2 for prod' }).click()

  await expect(confirm).toBeHidden()
  await expect(dialog.getByText('The environment moved since this screen was opened')).toBeVisible()
})

test('inspect shows the added-versus-predecessor diff and compiled schema', async ({ page }) => {
  await installApi(page)
  await page.goto('/admin/metadata')

  await page.getByRole('button', { name: 'Inspect revision M2' }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog.getByText('Revision M2')).toBeVisible()
  await expect(dialog.getByText('Added versus predecessor')).toBeVisible()
  await expect(dialog.getByText('Canonical definition')).toBeVisible()
  await expect(dialog.getByText('Compiled metadata schema')).toBeVisible()
  await dialog.getByRole('button', { name: 'Close' }).last().click()
  await expect(dialog).toBeHidden()
})

test('a failed configuration load keeps the page recoverable', async ({ page }) => {
  const state = await installApi(page)
  state.failEnvironments = true
  await page.goto('/admin/metadata')

  await expect(page.getByRole('alert').filter({ hasText: 'metadata table unavailable' })).toBeVisible()
  await expect(card(page, 'Protected defaults').getByText('idempotencyKey')).toBeVisible()
})

test('empty history guidance appears before any revision is saved', async ({ page }) => {
  await installApi(page)
  await page.route('**/dev/metadata/revisions**', async (route) => {
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ revisions: [] }),
    })
  })
  await page.goto('/admin/metadata')

  await expect(page.getByText('No revisions yet')).toBeVisible()
})
