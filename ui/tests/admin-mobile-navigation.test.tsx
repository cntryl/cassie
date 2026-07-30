import { describe, expect, it } from "vite-plus/test";

import { saveQueryWorkspace } from "@/features/query/query-tabs";
import { flushUi, mountQueryRoute } from "./support/query-page-harness";

describe("admin mobile navigation", () => {
  it("should_close_the_schema_sheet_after_query_activation_and_restore_trigger_focus", async () => {
    // Arrange
    saveQueryWorkspace("anonymous", {
      version: 1,
      activeTabId: "query-1",
      tabs: [
        {
          id: "query-1",
          ordinal: 1,
          title: "Query 1",
          database: "postgres",
          sql: "SELECT 1;",
        },
        {
          id: "query-2",
          ordinal: 2,
          title: "Query 2",
          database: "postgres",
          sql: "SELECT 2;",
        },
      ],
    });
    const root = await mountQueryRoute();
    const trigger = root.querySelector<HTMLButtonElement>('[aria-label="Open schema browser"]');
    if (!trigger) throw new Error("Missing mobile schema trigger");

    // Act
    trigger.click();
    await flushUi();
    const sheet = document.querySelector('[data-slot="sheet-content"]');
    const secondQuery = sheet?.querySelector<HTMLButtonElement>("#mobile-saved-query-query-2");
    if (!sheet || !secondQuery) throw new Error("Missing mobile schema sheet/query");
    secondQuery.click();
    await flushUi();
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

    // Assert
    expect(document.querySelector('[data-slot="sheet-content"][data-state="open"]')).toBeNull();
    expect(root.querySelector("#query-workspace-query-2")).not.toBeNull();
    expect(document.activeElement).toBe(trigger);
  });
});
