import { expect, test } from "@playwright/test";

test("should_present_logout_as_the_login_pages_companion", async ({ page }) => {
  // Arrange
  await page.goto("/login");
  await page.getByLabel("Username").fill("admin");
  await page.getByLabel("Password").fill("pwd123");
  await page.getByRole("button", { name: "Sign in" }).click();

  // Act
  await page.goto("/logout");
  const viewport = page.viewportSize();
  const pageBounds = await page.getByRole("main").boundingBox();
  const cardBounds = await page.locator('[data-slot="card"]').boundingBox();

  // Assert
  await expect(page.getByRole("heading", { name: "Sign out of Cassie Admin?" })).toBeVisible();
  await expect(page.getByText("You’re signed in as admin.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Sign out" })).toBeVisible();
  await expect(page.getByRole("link", { name: "Stay signed in" })).toBeVisible();
  expect(viewport).not.toBeNull();
  expect(pageBounds).not.toBeNull();
  expect(cardBounds).not.toBeNull();
  if (!viewport || !pageBounds || !cardBounds) return;

  expect(pageBounds.width).toBeGreaterThanOrEqual(viewport.width - 1);
  expect(pageBounds.height).toBeGreaterThanOrEqual(viewport.height - 1);
  expect(cardBounds.width).toBeLessThanOrEqual(384);
  expect(Math.abs(cardBounds.x + cardBounds.width / 2 - viewport.width / 2)).toBeLessThanOrEqual(1);
  expect(Math.abs(cardBounds.y + cardBounds.height / 2 - viewport.height / 2)).toBeLessThanOrEqual(
    1,
  );
});
