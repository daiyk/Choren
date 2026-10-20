import { defineConfig } from "@playwright/test";
export default defineConfig({
  testDir: "./test/ui",
  fullyParallel: true,
  workers: 2,
  timeout: 30_000,
  expect: { timeout: 7000 },
  use: {
    baseURL: "http://127.0.0.1:1420",
    viewport: { width: 1440, height: 900 },
    // Tests start past the first-run welcome window; welcome.spec.ts starts fresh.
    storageState: {
      cookies: [],
      origins: [{ origin: "http://127.0.0.1:1420", localStorage: [{ name: "choren.welcome", value: '"done"' }] }],
    },
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    { name: "webkit", use: { browserName: "webkit" } },
  ],
  webServer: {
    command: "npm run dev -- --host 127.0.0.1",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: !process.env.CI,
  },
});
