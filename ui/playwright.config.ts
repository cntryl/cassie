import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: false,
  forbidOnly: Boolean(process.env.CI),
  retries: process.env.CI ? 1 : 0,
  reporter: "line",
  use: {
    baseURL: "http://127.0.0.1:18080",
    trace: "retain-on-failure",
    locale: "en-US",
    timezoneId: "UTC",
    reducedMotion: "reduce",
  },
  webServer: {
    command:
      "CASSIE_REST_LISTEN=127.0.0.1:18080 CASSIE_PGWIRE_LISTEN=127.0.0.1:15432 CASSIE_ADMIN_PASSWORD=cassie-e2e-password CASSIE_MIDGE_ALLOW_FALLBACK=1 CASSIE_ADMIN_UI_DIR=./dist ../target/debug/cassie",
    cwd: ".",
    url: "http://127.0.0.1:18080/health",
    timeout: 120_000,
    reuseExistingServer: false,
  },
  projects: [
    {
      name: "desktop-chromium",
      use: { ...devices["Desktop Chrome"], viewport: { width: 1440, height: 900 } },
    },
    {
      name: "pixel-7",
      use: { ...devices["Pixel 7"] },
    },
  ],
});
