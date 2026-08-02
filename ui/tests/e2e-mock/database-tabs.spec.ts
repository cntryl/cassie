import { expect, type Page, test } from "@playwright/test";

async function expandTables(page: Page, database: string, namespace: string) {
  const databaseTree = page.locator(
    `[data-testid="query-schema-tree-database"][data-database="${database}"]`,
  );
  const databaseToggle = databaseTree.locator(":scope > button");
  if ((await databaseToggle.getAttribute("aria-expanded")) !== "true") {
    await databaseToggle.click();
  }
  await expect(databaseTree).toHaveAttribute("data-load-state", "loaded");

  const namespaceTree = databaseTree.locator(`[data-namespace="${namespace}"]`);
  const namespaceToggle = namespaceTree.locator(":scope > button");
  if ((await namespaceToggle.getAttribute("aria-expanded")) !== "true") {
    await namespaceToggle.click();
  }

  const tables = namespaceTree.locator('[data-section="tables"]');
  await tables.getByRole("button", { name: /^Tables / }).click();
}

test("should_keep_database_query_tabs_isolated_and_restore_drafts", async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (message) => message.type() === "error" && errors.push(message.text()));
  page.on("pageerror", (error) => errors.push(error.message));

  await page.goto("/login");
  await page.getByLabel("Username").fill("root");
  await page.getByLabel("Password").fill("pwd123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByText("Choose a database to open a query workspace.")).toBeVisible();
  errors.length = 0;

  await page.getByRole("button", { name: "New Query" }).first().click();
  await page.getByRole("dialog").getByRole("button", { name: "Database" }).click();
  await page.getByRole("option", { name: "Database1" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("button", { name: /Query 1 Database1/ })).toBeVisible();
  await expandTables(page, "Database1", "public");
  await expect(page.getByTestId("query-schema-tree")).toContainText("customers");
  await expect(page.getByTestId("query-schema-tree")).toContainText("orders");
  await expect(page.getByTestId("query-schema-tree")).toContainText("reporting");
  await expect(page.getByTestId("query-schema-tree")).toContainText("audit");
  const editorPanel = page.getByTestId("query-editor-panel");
  await expect(editorPanel).toBeVisible();
  const editorBounds = await editorPanel.boundingBox();
  expect(editorBounds).not.toBeNull();
  expect(editorBounds?.height).toBeGreaterThan(200);
  const database1Response = page.waitForResponse((response) =>
    response.url().includes("/api/v1/admin/query-executions"),
  );
  await page.locator("[data-query-page]:visible").getByRole("button", { name: "Run" }).click();
  const database1Result = await database1Response;
  expect(database1Result.status()).toBe(200);
  expect((await database1Result.json()).rows.length).toBeGreaterThan(0);

  await page.getByRole("button", { name: "New query" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Database" }).click();
  await page.getByRole("option", { name: "Database2" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Create" }).click();
  await expect(page.getByRole("button", { name: /Query 2 Database2/ })).toBeVisible();
  await expandTables(page, "Database2", "inventory");
  await expandTables(page, "Database2", "support");
  await expect(page.getByTestId("query-schema-tree")).toContainText("warehouses");
  await expect(page.getByTestId("query-schema-tree")).toContainText("tickets");
  await expect(page.getByTestId("query-schema-tree")).toContainText("inventory");
  await expect(page.getByTestId("query-schema-tree")).toContainText("support");
  const database2Response = page.waitForResponse((response) =>
    response.url().includes("/api/v1/admin/query-executions"),
  );
  await page.locator("[data-query-page]:visible").getByRole("button", { name: "Run" }).click();
  expect((await (await database2Response).json()).rows.length).toBeGreaterThan(0);
  await page.getByRole("button", { name: /Query 1 Database1/ }).click();
  await expect
    .poll(() =>
      page.evaluate(() => {
        const workspace = JSON.parse(
          sessionStorage.getItem("cassie.query-workspace.v1:root") ?? "null",
        );
        return workspace?.activeTabId === workspace?.tabs[0]?.id;
      }),
    )
    .toBe(true);
  await page.evaluate(() => {
    const key = "cassie.query-workspace.v1:root";
    const workspace = JSON.parse(sessionStorage.getItem(key) ?? "null");
    workspace.tabs[0].sql = "SELECT 'Database1' AS source;";
    workspace.tabs[1].sql = "SELECT 'Database2' AS source;";
    sessionStorage.setItem(key, JSON.stringify(workspace));
  });

  await page.reload();
  await expect(page.getByRole("button", { name: /Query 1 Database1/ })).toBeVisible();
  await expect(page.getByRole("button", { name: /Query 2 Database2/ })).toBeVisible();
  await expect(page.locator("[data-query-page]:visible").locator(".monaco-editor")).toContainText(
    "Database1",
  );
  const stored = await page.evaluate(() =>
    sessionStorage.getItem("cassie.query-workspace.v1:root"),
  );
  expect(stored).toContain("SELECT 'Database1' AS source;");
  expect(errors).toEqual([]);
});

test("should_keep_the_database_tree_visible_and_create_a_database", async ({ page }, testInfo) => {
  // Arrange
  const databaseName = `reporting_${testInfo.project.name.replace(/-/g, "_")}`;
  await page.goto("/login");
  await page.getByLabel("Username").fill("root");
  await page.getByLabel("Password").fill("pwd123");
  await page.getByRole("button", { name: "Sign in" }).click();

  // Act / Assert: the database tree exists before a query does.
  const tree = page.getByTestId("query-schema-tree");
  const sidebar = page.getByRole("complementary", { name: "Schema browser" });
  const sidebarFooter = page.getByTestId("admin-sidebar-footer");
  await expect(tree).toBeVisible();
  await expect(tree.getByText("Database1", { exact: true })).toBeVisible();
  await expect(tree.getByText("Database2", { exact: true })).toBeVisible();
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
  const dialog = page.getByRole("dialog");
  await dialog.evaluate(async (element) => {
    await Promise.all(element.getAnimations().map((animation) => animation.finished));
  });
  const viewport = page.viewportSize();
  const dialogBounds = await dialog.boundingBox();
  expect(viewport).not.toBeNull();
  expect(dialogBounds).not.toBeNull();
  if (viewport && dialogBounds) {
    expect(Math.abs(dialogBounds.x + dialogBounds.width / 2 - viewport.width / 2)).toBeLessThan(2);
    expect(Math.abs(dialogBounds.y + dialogBounds.height / 2 - viewport.height / 2)).toBeLessThan(
      2,
    );
  }
  await page.getByLabel("Database name").fill(databaseName);
  const response = page.waitForResponse(
    (candidate) =>
      candidate.url().includes("/api/v1/admin/databases") &&
      candidate.request().method() === "POST",
  );
  await page.getByRole("dialog").getByRole("button", { name: "Create database" }).click();

  // Assert
  expect((await response).status()).toBe(201);
  await expect(page.getByRole("button", { name: `Query 1 ${databaseName}` })).toBeVisible();
  await expect(
    tree.locator(".cassie-query-schema-database-label", { hasText: databaseName }),
  ).toBeVisible();
});
