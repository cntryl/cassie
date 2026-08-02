import { describe, expect, it } from "vite-plus/test";

import { flushUi, mountQueryRoute } from "./support/query-page-harness";

describe("admin mobile navigation", () => {
  it("should_toggle_schema_browser_from_mobile_control", async () => {
    // Arrange
    const root = await mountQueryRoute();
    const trigger = root.querySelector<HTMLButtonElement>('[aria-label="Toggle schema browser"]');
    const sidebar = root.querySelector<HTMLElement>('[aria-label="Schema browser"]');
    if (!trigger) throw new Error("Missing mobile schema trigger");
    if (!sidebar) throw new Error("Missing schema browser");

    // Act
    trigger.click();
    await flushUi();

    // Assert
    expect(sidebar.getAttribute("data-mobile-open")).toBeNull();
  });
});
