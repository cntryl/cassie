import { expect, type Page, test } from "@playwright/test";

async function openAnalyticsQuery(page: Page) {
  await page.goto("/login");
  await page.getByLabel("Username").fill("admin");
  await page.getByLabel("Password").fill("pwd123");
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.getByRole("button", { name: "New Query" }).first().click();
  await page
    .getByRole("dialog")
    .getByRole("button", { name: /analytics/ })
    .click();

  const editor = page.locator("[data-query-page]:visible .monaco-editor");
  await expect(editor).toBeVisible({ timeout: 15_000 });
  await editor.click({ position: { x: 160, y: 20 } });
  return editor;
}

async function expectEditorFocused(editor: ReturnType<Page["locator"]>) {
  expect(await editor.evaluate((element) => element.contains(document.activeElement))).toBe(true);
}

test("should_preserve_focus_selection_and_history_while_editing_sql", async ({ page }) => {
  // Arrange
  const editor = await openAnalyticsQuery(page);

  // Act / Assert: consecutive input must not lose focus.
  await page.keyboard.type(" abc");
  await expect(editor).toContainText("abc");
  await expectEditorFocused(editor);

  // Command+A and Backspace must clear the complete model.
  await page.keyboard.press("Meta+A");
  await page.keyboard.press("Backspace");
  await expect(editor.locator(".view-lines")).toHaveText("");
  await expectEditorFocused(editor);

  // Undo and redo retain a usable editor and model history.
  await page.keyboard.press("Meta+Z");
  await expect(editor).toContainText("abc");
  await page.keyboard.press("Meta+Shift+Z");
  await expect(editor.locator(".view-lines")).toHaveText("");
  await expectEditorFocused(editor);

  // Ctrl+A is supported as well, including a one-character draft.
  await page.keyboard.insertText("x");
  await page.keyboard.press("Control+A");
  await page.keyboard.press("Delete");
  await expect(editor.locator(".view-lines")).toHaveText("");
});

test("should_persist_multiline_sql_entered_through_monaco", async ({ page }) => {
  // Arrange
  await openAnalyticsQuery(page);
  const sql = "SELECT 42 AS answer;\nSELECT 'saved' AS state;";

  // Act
  await page.keyboard.press("Meta+A");
  await page.keyboard.insertText(sql);

  // Assert
  await expect
    .poll(() => page.evaluate(() => localStorage.getItem("cassie.query-workspace.v1:admin") ?? ""))
    .toContain("SELECT 42 AS answer;");
  await page.reload();
  const restoredEditor = page.locator("[data-query-page]:visible .monaco-editor");
  await expect(restoredEditor).toContainText("SELECT 42 AS answer;");
  await expect(restoredEditor).toContainText("SELECT 'saved' AS state;");
});

test("should_keep_editor_usable_when_query_actions_run", async ({ page }) => {
  // Arrange
  const editor = await openAnalyticsQuery(page);
  const runButton = page.getByRole("button", { name: "Run" });
  await expect(runButton).toBeEnabled();
  await expect(page.locator(".cassie-query-availability-status")).toHaveCount(0);
  await page.keyboard.press("Meta+A");
  await page.keyboard.insertText("  SELECT 1 AS ready;  ");
  const panel = page.getByTestId("query-editor-panel");
  const initialBounds = await panel.boundingBox();
  expect(initialBounds).not.toBeNull();

  // Act / Assert
  await page.getByRole("button", { name: "Trim" }).click();
  await expect(editor).toContainText("SELECT 1 AS ready;");

  for (const action of ["Validate", "Explain", "Run"] as const) {
    const response = page.waitForResponse((candidate) =>
      candidate
        .url()
        .includes(
          action === "Validate"
            ? "/query-validations"
            : action === "Explain"
              ? "/query-explanations"
              : "/query-executions",
        ),
    );
    await page.getByRole("button", { name: action }).click();
    expect((await response).status()).toBe(200);
    await expect(runButton).toBeEnabled();
    const bounds = await panel.boundingBox();
    expect(bounds).not.toBeNull();
    expect(bounds?.height, `${action} collapsed the editor`).toBeGreaterThan(200);
    expect(
      Math.abs((bounds?.y ?? 0) - (initialBounds?.y ?? 0)),
      `${action} changed editor position`,
    ).toBeLessThan(2);
  }
});

test("should_offer_sql_and_schema_autocomplete", async ({ page }) => {
  // Arrange
  const editor = await openAnalyticsQuery(page);
  await expect(page.getByText("events", { exact: true }).first()).toBeVisible();

  // Act / Assert: SQL keyword completion.
  await page.keyboard.press("Meta+A");
  await page.keyboard.insertText("SEL");
  await page.keyboard.press("Control+Space");
  const suggestions = page.locator(".suggest-widget.visible");
  await expect(suggestions).toBeVisible();
  await expect(suggestions).toContainText("SELECT");

  // Act / Assert: loaded schema completion.
  await page.keyboard.press("Escape");
  await page.keyboard.press("Meta+A");
  await page.keyboard.insertText("eve");
  await page.keyboard.press("Control+Space");
  await expect(suggestions).toBeVisible();
  await expect(suggestions).toContainText("analytics.public.events");
  await expectEditorFocused(editor);
});
