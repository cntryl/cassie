import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";

async function expectNoAccessibilityViolations(page: Page) {
  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa", "best-practice"])
    .analyze();
  expect(results.violations).toEqual([]);
}

test("should_have_no_accessibility_violations_in_core_query_states", async ({ page }) => {
  // Arrange
  await page.goto("/login");
  await page.addStyleTag({
    content:
      "*, *::before, *::after { animation: none !important; transition: none !important; caret-color: transparent !important; }",
  });

  // Act / Assert
  await expectNoAccessibilityViolations(page);
  await page.getByLabel("Username").fill("root");
  await page.getByLabel("Password").fill("pwd123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByText("Choose a database to open a query workspace.")).toBeVisible();
  await expectNoAccessibilityViolations(page);

  await page.getByRole("button", { name: "New Query" }).first().click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await expect(page.getByRole("dialog")).toHaveCSS("opacity", "1");
  await expectNoAccessibilityViolations(page);

  await page.getByRole("dialog").getByRole("button", { name: "Database" }).click();
  await page.getByRole("option", { name: "Database1" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Create" }).click();
  if ((page.viewportSize()?.width ?? 0) < 768) {
    await page.getByRole("button", { name: "Toggle schema browser" }).click();
  }
  await expect(page.getByRole("button", { name: /Query 1 Database1/ })).toBeVisible();
  await expectNoAccessibilityViolations(page);

  const executionResponse = page.waitForResponse((response) =>
    response.url().includes("/api/v1/admin/query-executions"),
  );
  await page.locator("[data-query-page]:visible").getByRole("button", { name: "Run" }).click();
  expect((await executionResponse).status()).toBe(200);
  await expect(page.getByLabel("Execution summary")).toContainText("SELECT");
  await expectNoAccessibilityViolations(page);

  await page.route("**/api/v1/admin/query-executions", async (route) => {
    await route.fulfill({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({ error: "simulated query failure" }),
    });
  });
  await page.locator("[data-query-page]:visible").getByRole("button", { name: "Run" }).click();
  await expect(page.getByText("Query action failed")).toBeVisible();
  await expectNoAccessibilityViolations(page);
});

test("should_have_no_accessibility_violations_on_the_not_found_page", async ({ page }) => {
  // Arrange / Act
  await page.goto("/missing-page");

  // Assert
  await expect(page.getByRole("main")).toHaveCount(1);
  await expect(page.getByRole("heading", { name: "Page not found", level: 1 })).toBeVisible();
  await expect(page.getByRole("link", { name: "Return to query workspace" })).toBeVisible();
  await expectNoAccessibilityViolations(page);
});

test("should_have_no_accessibility_violations_while_signing_out", async ({ page }) => {
  // Arrange
  await page.goto("/login");
  await page.getByLabel("Username").fill("root");
  await page.getByLabel("Password").fill("pwd123");
  await page.getByRole("button", { name: "Sign in" }).click();
  let releaseLogout!: () => void;
  const logoutReleased = new Promise<void>((resolve) => {
    releaseLogout = resolve;
  });
  await page.route("**/api/v1/auth/logout", async (route) => {
    await logoutReleased;
    await route.continue();
  });

  // Act
  await page.goto("/logout");

  // Assert
  try {
    await expect(page.getByRole("heading", { name: "Signing out", level: 1 })).toBeVisible();
    await expectNoAccessibilityViolations(page);
  } finally {
    releaseLogout();
  }
  await expect(page).toHaveURL(/\/login$/);
});
