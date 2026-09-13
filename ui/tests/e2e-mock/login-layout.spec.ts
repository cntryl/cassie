import { expect, test } from "@playwright/test";

test("should_center_the_login_panel_in_the_full_viewport", async ({ page }) => {
  // Arrange
  await page.goto("/login");
  const viewport = page.viewportSize();

  // Act
  const pageBounds = await page.getByRole("main").boundingBox();
  const pagePadding = await page.getByRole("main").evaluate((element) => {
    const styles = getComputedStyle(element);
    return { inlineEnd: styles.paddingRight, inlineStart: styles.paddingLeft };
  });
  const panelBounds = await page.locator(".cassie-auth-panel").boundingBox();

  // Assert
  expect(viewport).not.toBeNull();
  expect(pageBounds).not.toBeNull();
  expect(panelBounds).not.toBeNull();
  if (!viewport || !pageBounds || !panelBounds) return;

  const expectedPadding = viewport.width >= 768 ? 24 : 16;
  expect(pagePadding).toEqual({
    inlineEnd: `${expectedPadding}px`,
    inlineStart: `${expectedPadding}px`,
  });
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
});

test("should_preserve_mobile_padding_around_the_login_panel", async ({ page }) => {
  // Arrange
  await page.setViewportSize({ width: 360, height: 800 });
  await page.goto("/login");

  // Act
  const pagePadding = await page.getByRole("main").evaluate((element) => {
    const styles = getComputedStyle(element);
    return { inlineEnd: styles.paddingRight, inlineStart: styles.paddingLeft };
  });
  const panelBounds = await page.locator(".cassie-auth-panel").boundingBox();

  // Assert
  expect(pagePadding).toEqual({ inlineEnd: "16px", inlineStart: "16px" });
  expect(panelBounds).not.toBeNull();
  expect(panelBounds?.x).toBe(16);
  expect(panelBounds?.width).toBe(328);
});
