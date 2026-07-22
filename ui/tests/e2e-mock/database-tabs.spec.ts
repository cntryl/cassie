import { expect, test } from "@playwright/test";

test("should_keep_database_query_tabs_isolated_and_restore_drafts", async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (message) => message.type() === "error" && errors.push(message.text()));
  page.on("pageerror", (error) => errors.push(error.message));

  await page.goto("/login");
  await page.getByLabel("Username").fill("admin");
  await page.getByLabel("Password").fill("pwd123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByText("Choose a database to open a query workspace.")).toBeVisible();
  errors.length = 0;

  await page.getByRole("button", { name: "New Query" }).first().click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: /analytics/ })
    .click();
  await expect(page.getByRole("tab", { name: /Query 1 analytics/ })).toBeVisible();
  const analyticsResponse = page.waitForResponse((response) =>
    response.url().includes("/api/v1/admin/query-executions"),
  );
  await page
    .getByRole("tabpanel", { name: /Query 1 analytics/ })
    .getByRole("button", { name: "Run" })
    .click();
  const analyticsResult = await analyticsResponse;
  expect(analyticsResult.status()).toBe(200);
  expect((await analyticsResult.json()).rows.length).toBeGreaterThan(0);

  await page.getByRole("button", { name: "New Query" }).click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: /postgres/ })
    .click();
  await expect(page.getByRole("tab", { name: /Query 2 postgres/ })).toBeVisible();
  await expect(page.getByRole("tabpanel", { name: /Query 2 postgres/ })).toBeVisible();
  const postgresResponse = page.waitForResponse((response) =>
    response.url().includes("/api/v1/admin/query-executions"),
  );
  await page
    .getByRole("tabpanel", { name: /Query 2 postgres/ })
    .getByRole("button", { name: "Run" })
    .click();
  expect((await (await postgresResponse).json()).rows.length).toBeGreaterThan(0);
  await page.getByRole("tab", { name: /Query 1 analytics/ }).click();
  await page.evaluate(() => {
    const key = "cassie.query-workspace.v1:admin";
    const workspace = JSON.parse(localStorage.getItem(key) ?? "null");
    workspace.tabs[0].sql = "SELECT 'analytics' AS source;";
    workspace.tabs[1].sql = "SELECT 'postgres' AS source;";
    localStorage.setItem(key, JSON.stringify(workspace));
  });

  await page.reload();
  await expect(page.getByRole("tab", { name: /Query 1 analytics/ })).toBeVisible();
  await expect(page.getByRole("tab", { name: /Query 2 postgres/ })).toBeVisible();
  await expect(
    page.getByRole("tabpanel", { name: /Query 1 analytics/ }).locator(".monaco-editor"),
  ).toContainText("analytics");
  const stored = await page.evaluate(() => localStorage.getItem("cassie.query-workspace.v1:admin"));
  expect(stored).toContain("SELECT 'analytics' AS source;");
  expect(errors).toEqual([]);
});
