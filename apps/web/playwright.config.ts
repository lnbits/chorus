import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: 'tests',
  fullyParallel: false,
  workers: 1,
  timeout: 45000,
  use: {
    baseURL: process.env.CHORUS_E2E_URL || 'http://127.0.0.1:8787',
    browserName: 'chromium',
    launchOptions: {
      executablePath: process.env.CHORUS_CHROME,
      args: ['--no-sandbox', '--disable-dev-shm-usage']
    },
    screenshot: 'only-on-failure'
  },
  reporter: 'list'
});
