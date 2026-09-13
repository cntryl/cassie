import { expect, test } from "@playwright/test";

test("should_present_logout_as_the_login_pages_companion", async ({ page }) => {
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
  const viewport = page.viewportSize();
  const pageBounds = await page.getByRole("main").boundingBox();
  const panelBounds = await page.locator(".cassie-auth-panel").boundingBox();

  // Assert
  await expect(page.getByRole("heading", { name: "Signing out" })).toBeVisible();
  await expect(page.getByText("Clearing your Cassie Admin session.")).toBeVisible();
  expect(viewport).not.toBeNull();
  expect(pageBounds).not.toBeNull();
  expect(panelBounds).not.toBeNull();
  if (!viewport || !pageBounds || !panelBounds) return;

  const expectedPadding = viewport.width >= 768 ? 24 : 16;
  expect(pageBounds.width).toBeGreaterThanOrEqual(viewport.width - 1);
  expect(pageBounds.height).toBeGreaterThanOrEqual(viewport.height - 1);
  expect(panelBounds.width).toBe(Math.min(384, viewport.width - expectedPadding * 2));
  expect(Math.abs(panelBounds.x + panelBounds.width / 2 - viewport.width / 2)).toBeLessThanOrEqual(
    1,
  );
  expect(
    Math.abs(panelBounds.y + panelBounds.height / 2 - viewport.height / 2),
  ).toBeLessThanOrEqual(1);
  await expect(page.locator(".cassie-auth-panel img.cassie-brand-logo")).toBeVisible();
  await expect(page.getByLabel("Toggle color theme")).toBeVisible();
  await expect(page.locator('[data-slot="card"]')).toHaveCount(0);
  await expect(page.locator("header, footer")).toHaveCount(0);

  releaseLogout();
  await expect(page).toHaveURL(/\/login$/);
  await expect(page.getByRole("heading", { name: "Sign in" })).toBeVisible();
});
