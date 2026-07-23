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
  await expect(page.getByRole("button", { name: /Query 1 analytics/ })).toBeVisible();
  const editorPanel = page.getByTestId("query-editor-panel");
  await expect(editorPanel).toBeVisible();
  const editorBounds = await editorPanel.boundingBox();
  expect(editorBounds).not.toBeNull();
  expect(editorBounds?.height).toBeGreaterThan(200);
  const analyticsResponse = page.waitForResponse((response) =>
    response.url().includes("/api/v1/admin/query-executions"),
  );
  await page.locator("[data-query-page]:visible").getByRole("button", { name: "Run" }).click();
  const analyticsResult = await analyticsResponse;
  expect(analyticsResult.status()).toBe(200);
  expect((await analyticsResult.json()).rows.length).toBeGreaterThan(0);

  await page.getByRole("button", { name: "New query" }).click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: /postgres/ })
    .click();
  await expect(page.getByRole("button", { name: /Query 2 postgres/ })).toBeVisible();
  const postgresResponse = page.waitForResponse((response) =>
    response.url().includes("/api/v1/admin/query-executions"),
  );
  await page.locator("[data-query-page]:visible").getByRole("button", { name: "Run" }).click();
  expect((await (await postgresResponse).json()).rows.length).toBeGreaterThan(0);
  await page.getByRole("button", { name: /Query 1 analytics/ }).click();
  await page.evaluate(() => {
    const key = "cassie.query-workspace.v1:admin";
    const workspace = JSON.parse(localStorage.getItem(key) ?? "null");
    workspace.tabs[0].sql = "SELECT 'analytics' AS source;";
    workspace.tabs[1].sql = "SELECT 'postgres' AS source;";
    localStorage.setItem(key, JSON.stringify(workspace));
  });

  await page.reload();
  await expect(page.getByRole("button", { name: /Query 1 analytics/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Query 2 postgres/ })).toBeVisible();
  await expect(page.locator("[data-query-page]:visible").locator(".monaco-editor")).toContainText(
    "analytics",
  );
  const stored = await page.evaluate(() => localStorage.getItem("cassie.query-workspace.v1:admin"));
  expect(stored).toContain("SELECT 'analytics' AS source;");
  expect(errors).toEqual([]);
});

test("should_keep_the_database_tree_visible_and_create_a_database", async ({ page }) => {
  // Arrange
  await page.goto("/login");
  await page.getByLabel("Username").fill("admin");
  await page.getByLabel("Password").fill("pwd123");
  await page.getByRole("button", { name: "Sign in" }).click();

  // Act / Assert: the database tree exists before a query does.
  const tree = page.getByTestId("query-schema-tree");
  const sidebar = page.getByRole("complementary", { name: "Schema browser" });
  const sidebarFooter = page.getByTestId("admin-sidebar-footer");
  await expect(tree).toBeVisible();
  await expect(tree.getByText("analytics", { exact: true })).toBeVisible();
  await expect(tree.getByText("postgres", { exact: true })).toBeVisible();
  await expect(page.locator(".cassie-admin-header")).toHaveCount(0);
  await expect(sidebar.getByLabel("Cassie admin home")).toBeVisible();
  await expect(sidebarFooter.getByLabel("Toggle color theme")).toBeVisible();
  await expect(sidebarFooter.getByLabel("Sign out")).toBeVisible();
  const sidebarBounds = await sidebar.boundingBox();
  const footerBounds = await sidebarFooter.boundingBox();
  const shellViewport = page.viewportSize();
  expect(sidebarBounds?.y).toBe(0);
  if (shellViewport && shellViewport.width >= 768) {
    expect(sidebarBounds?.height).toBe(shellViewport.height);
    expect(
      Math.abs((footerBounds?.y ?? 0) + (footerBounds?.height ?? 0) - shellViewport.height + 8),
    ).toBeLessThan(2);
  }

  await tree.getByRole("button", { name: "Create database" }).click();
  const viewport = page.viewportSize();
  const dialogBounds = await page.getByRole("dialog").boundingBox();
  expect(viewport).not.toBeNull();
  expect(dialogBounds).not.toBeNull();
  if (viewport && dialogBounds) {
    expect(Math.abs(dialogBounds.x + dialogBounds.width / 2 - viewport.width / 2)).toBeLessThan(2);
    expect(Math.abs(dialogBounds.y + dialogBounds.height / 2 - viewport.height / 2)).toBeLessThan(
      2,
    );
  }
  await page.getByLabel("Database name").fill("reporting");
  const response = page.waitForResponse(
    (candidate) =>
      candidate.url().includes("/api/v1/admin/query-executions") &&
      candidate.request().method() === "POST",
  );
  await page.getByRole("dialog").getByRole("button", { name: "Create database" }).click();

  // Assert
  expect((await response).status()).toBe(200);
  await expect(page.getByRole("button", { name: /Query 1 reporting/ })).toBeVisible();
  await expect(
    tree.locator(".cassie-query-schema-database-label", { hasText: "reporting" }),
  ).toBeVisible();
});
