import { expect, test, type Page } from "@playwright/test";

async function openPopulatedWorkspace(page: Page) {
  await page.emulateMedia({ colorScheme: "light", reducedMotion: "reduce" });
  await page.goto("/login");
  await page.getByLabel("Username").fill("root");
  await page.getByLabel("Password").fill("pwd123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.getByRole("button", { name: "New Query" }).first().click();
  await page.getByRole("dialog").getByRole("button", { name: "Database" }).click();
  await page.getByRole("option", { name: "Database1" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Create" }).click();

  const response = page.waitForResponse((candidate) =>
    candidate.url().includes("/api/v1/admin/query-executions"),
  );
  await page.locator("[data-query-page]:visible").getByRole("button", { name: "Run" }).click();
  expect((await response).status()).toBe(200);
  await expect(page.getByLabel("Execution summary")).toContainText("SELECT");
  await expect(page.locator(".monaco-editor")).toBeVisible();
  await page.addStyleTag({
    content:
      "*, *::before, *::after { animation: none !important; transition: none !important; caret-color: transparent !important; }",
  });
}

test("should_match_populated_admin_shell_visual_states", async ({ page }, testInfo) => {
  await openPopulatedWorkspace(page);

  const mobile = testInfo.project.name === "pixel-7";
  const toggle = page.getByRole("button", { name: "Toggle schema browser" });
  if (mobile) {
    await expect(toggle).toHaveAttribute("aria-expanded", "false");
    await expect(page).toHaveScreenshot("admin-populated-mobile-closed.png", {
      fullPage: true,
      maxDiffPixelRatio: 0.02,
    });

    await toggle.click();
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    await expect(page).toHaveScreenshot("admin-populated-mobile-open.png", {
      fullPage: true,
      maxDiffPixelRatio: 0.02,
    });
    return;
  }

  await expect(page).toHaveScreenshot("admin-populated-desktop-light.png", {
    fullPage: true,
    maxDiffPixelRatio: 0.02,
  });
  await page.getByRole("button", { name: "Toggle color theme" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(page.getByLabel("Execution summary")).toContainText("SELECT");
  await expect(page.getByRole("table")).toBeVisible();
  await expect(page).toHaveScreenshot("admin-populated-desktop-dark.png", {
    fullPage: true,
    maxDiffPixelRatio: 0.02,
  });
});
