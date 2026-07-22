import { defineConfig, devices } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/e2e-mock",
  fullyParallel: false,
  forbidOnly: Boolean(process.env.CI),
  reporter: "line",
  expect: { toHaveScreenshot: { maxDiffPixelRatio: 0.001 } },
  use: {
    baseURL: "http://127.0.0.1:14174",
    trace: "retain-on-failure",
    locale: "en-US",
    timezoneId: "UTC",
    reducedMotion: "reduce",
  },
  webServer: {
    command: "npm run dev:mock -- --host 127.0.0.1 --port 14174",
    url: "http://127.0.0.1:14174/login",
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
