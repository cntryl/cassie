import { expect, test } from "@playwright/test";

test("should_center_the_login_card_in_the_full_viewport", async ({ page }) => {
  // Arrange
  await page.goto("/login");
  const viewport = page.viewportSize();

  // Act
  const pageBounds = await page.getByRole("main").boundingBox();
  const cardBounds = await page.locator('[data-slot="card"]').boundingBox();

  // Assert
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
