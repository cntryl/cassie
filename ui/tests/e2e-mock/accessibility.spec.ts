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
  await page.getByLabel("Username").fill("admin");
  await page.getByLabel("Password").fill("pwd123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByText("Choose a database to open a query workspace.")).toBeVisible();
  await expectNoAccessibilityViolations(page);

  await page.getByRole("button", { name: "New Query" }).first().click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await expect(page.getByRole("dialog")).toHaveCSS("opacity", "1");
  await expectNoAccessibilityViolations(page);

  await page
    .getByRole("dialog")
    .getByRole("button", { name: /postgres/ })
    .click();
  await expect(page.getByRole("tab", { name: /Query 1 postgres/ })).toBeVisible();
  await expectNoAccessibilityViolations(page);
});
