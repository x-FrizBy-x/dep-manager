import { expect, test, type Page } from '@playwright/test'
import { calls, installBackend, plan, record } from './backend'

const PAGES = [
  'Overview',
  'Installed Software',
  'Updates',
  'Package Explorer',
  'Dependencies',
  'Project Dependencies',
  'Environment Backup',
  'Operation History',
  'Settings',
]

/** Collects uncaught errors and console errors so a page that renders but logs a failure still fails. */
function watchErrors(page: Page) {
  const errors: string[] = []
  page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`))
  page.on('console', (m) => m.type() === 'error' && errors.push(`console: ${m.text()}`))
  return errors
}

const nav = (page: Page, label: string) => page.getByRole('navigation', { name: 'Main' }).getByRole('button', { name: label, exact: true })

async function open(page: Page, overrides: Record<string, unknown> = {}) {
  await installBackend(page, overrides)
  await page.goto('/')
  await expect(nav(page, 'Overview')).toBeVisible()
}

test('every page opens without errors', async ({ page }) => {
  const errors = watchErrors(page)
  await open(page)
  for (const label of PAGES) {
    await nav(page, label).click()
    await expect(nav(page, label)).toHaveAttribute('aria-current', 'page')
    await expect(page.getByText('Loading…')).toHaveCount(0)
  }
  expect(errors).toEqual([])
})

test('installed list filters by search and shows unknown values as a dash', async ({ page }) => {
  await open(page)
  await nav(page, 'Installed Software').click()
  await expect(page.getByRole('cell', { name: 'typescript' })).toBeVisible()
  await expect(page.getByRole('cell', { name: 'left-pad' })).toBeVisible()

  await page.getByLabel('Search installed software').fill('left')
  await expect(page.getByRole('cell', { name: 'left-pad' })).toBeVisible()
  await expect(page.getByRole('cell', { name: 'typescript' })).toHaveCount(0)

  await page.getByRole('cell', { name: 'left-pad' }).click()
  const details = page.getByRole('complementary', { name: 'Package details' })
  await expect(details.getByText('1.3.0').first()).toBeVisible()
  // Fields the manager did not report are shown as unknown, never invented.
  await expect(details.locator('dt', { hasText: 'License' }).locator('xpath=following-sibling::dd')).toHaveText('—')
})

test('uninstall needs a preview and an explicit confirmation', async ({ page }) => {
  await open(page, { preview_operation: plan('uninstall', 'left-pad'), run_operation: record('uninstall', 'left-pad') })
  await nav(page, 'Installed Software').click()
  await page.getByRole('cell', { name: 'left-pad' }).click()

  await page.getByRole('button', { name: 'Uninstall' }).click()
  const dialog = page.getByRole('dialog')
  await expect(dialog).toContainText('npm uninstall -g left-pad') // exact command is shown first
  await expect(dialog).toContainText('Not required') // elevation is stated
  expect(await calls(page, 'preview_operation')).toHaveLength(1)
  expect(await calls(page, 'run_operation')).toHaveLength(0)

  await dialog.getByRole('button', { name: 'Cancel' }).click()
  await expect(dialog).toHaveCount(0)
  expect(await calls(page, 'run_operation')).toHaveLength(0)

  await page.getByRole('button', { name: 'Uninstall' }).click()
  await page.keyboard.press('Escape')
  await expect(page.getByRole('dialog')).toHaveCount(0)
  expect(await calls(page, 'run_operation')).toHaveLength(0)

  await page.getByRole('button', { name: 'Uninstall' }).click()
  await page.getByRole('button', { name: 'Run uninstall' }).click()
  await expect.poll(async () => (await calls(page, 'run_operation')).length).toBe(1)
  expect((await calls(page, 'run_operation'))[0].args).toEqual({ planId: 'plan-uninstall-left-pad' })
})

test('a failed preview shows the error and runs nothing', async ({ page }) => {
  await open(page, { preview_operation: { __error: 'refusing to uninstall npm itself' } })
  await nav(page, 'Installed Software').click()
  await page.getByRole('cell', { name: 'left-pad' }).click()
  await page.getByRole('button', { name: 'Uninstall' }).click()
  await expect(page.getByRole('alert')).toContainText('refusing to uninstall npm itself')
  await expect(page.getByRole('dialog')).toHaveCount(0)
  expect(await calls(page, 'run_operation')).toHaveLength(0)
})

test('managers without write support are read-only', async ({ page }) => {
  await open(page)
  await nav(page, 'Installed Software').click()
  await page.getByRole('cell', { name: 'node', exact: true }).click()
  const details = page.getByRole('complementary', { name: 'Package details' })
  await expect(details).toContainText('read-only')
  await expect(details.getByRole('button', { name: 'Uninstall' })).toHaveCount(0)
})

test('the update button offers the exact target version', async ({ page }) => {
  await open(page)
  await nav(page, 'Installed Software').click()
  await page.getByRole('cell', { name: 'typescript' }).click()
  await expect(page.getByRole('button', { name: 'Update to 5.5.0' })).toBeVisible()
})
