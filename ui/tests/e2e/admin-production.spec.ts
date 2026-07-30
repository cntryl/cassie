import { expect, test } from "@playwright/test";

test("should_run_the_admin_workflow_against_the_production_server", async ({ page, request }) => {
  // Arrange
  const consoleErrors: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });
  page.on("pageerror", (error) => consoleErrors.push(error.message));

  // Act: protected deep links continue through login.
  await page.goto("/?source=e2e");
  await expect(page).toHaveURL(/\/login\?next=/);
  await page.getByLabel("Username").fill("root");
  await page.getByLabel("Password").fill("wrong-password");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByText("The username or password is incorrect.")).toBeVisible();

  await page.getByLabel("Password").fill("cassie-e2e-password");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page).toHaveURL(/\/\?source=e2e/);
  await page.getByRole("button", { name: "New Query" }).first().click();
  await page.getByRole("dialog").getByRole("button", { name: "Database" }).click();
  await page.getByRole("option", { name: "postgres" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Create" }).click();
  await expect(page.locator('[data-query-editor="monaco"]').first()).toBeVisible();
  await page.waitForTimeout(250);
  consoleErrors.length = 0;

  await page.getByRole("button", { name: /Run/ }).first().click();
  await expect(page.getByText("SELECT").first()).toBeVisible();

  // Assert production CSP and SPA fallback are served by Cassie itself.
  const response = await request.get("/query/deep-link");
  expect(response.status()).toBe(200);
  expect(response.headers()["content-security-policy"]).toContain("worker-src 'self' blob:");
  expect(consoleErrors).toEqual([]);
});
