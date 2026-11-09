// Attaches to the running UniManage window (WebView2) over CDP.
// Start the app first with:
//   $env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = "--remote-debugging-port=9222"; npm run tauri dev
// Usage: node scripts/ui-check.mjs <screenshot-path> [click-text ...]
import { chromium } from 'playwright-core'

const [shot = 'ui.png', ...clicks] = process.argv.slice(2)
const browser = await chromium.connectOverCDP('http://localhost:9222')
const page = browser.contexts().flatMap((c) => c.pages())[0]
if (!page) throw new Error('no page found')

const problems = []
page.on('console', (m) => ['error', 'warning'].includes(m.type()) && problems.push(`${m.type()}: ${m.text()}`))
page.on('pageerror', (e) => problems.push(`pageerror: ${e.message}`))

for (const text of clicks) {
  await page.getByText(text, { exact: false }).first().click()
  await page.waitForTimeout(1500)
}
await page.screenshot({ path: shot })
console.log('title:', await page.title())
console.log('text:', (await page.locator('body').innerText()).slice(0, 1500))
console.log('problems:', problems.length ? problems : 'none')
await browser.close()
