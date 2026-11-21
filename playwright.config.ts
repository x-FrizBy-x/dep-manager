import { defineConfig } from '@playwright/test'

// Browser-only UI tests against the Vite dev server with a fake Tauri backend (see e2e/backend.ts).
export default defineConfig({
  testDir: './e2e',
  fullyParallel: true,
  reporter: [['list']],
  use: { baseURL: 'http://localhost:1420', trace: 'retain-on-failure' },
  webServer: {
    command: 'npm run dev',
    url: 'http://localhost:1420',
    reuseExistingServer: true,
    timeout: 60_000,
  },
})
