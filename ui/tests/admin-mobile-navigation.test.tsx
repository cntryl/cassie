import { describe, expect, it } from "vite-plus/test";

import { flushUi, mountQueryRoute } from "./support/query-page-harness";

describe("admin mobile navigation", () => {
  it("should_start_closed_and_toggle_schema_browser_from_mobile_control", async () => {
    // Arrange
    const root = await mountQueryRoute();
    const trigger = root.querySelector<HTMLButtonElement>('[aria-label="Toggle schema browser"]');
    const sidebar = root.querySelector<HTMLElement>('[aria-label="Schema browser"]');
    if (!trigger) throw new Error("Missing mobile schema trigger");
    if (!sidebar) throw new Error("Missing schema browser");

    // Assert initial state
    expect(sidebar.getAttribute("data-mobile-open")).toBeNull();
    expect(trigger.getAttribute("aria-expanded")).toBe("false");

    // Act: first toggle opens the browser.
    trigger.click();
    await flushUi();

    // Assert
    expect(sidebar.getAttribute("data-mobile-open")).toBe("true");
    expect(trigger.getAttribute("aria-expanded")).toBe("true");

    // Act: second toggle closes it again.
    trigger.click();
    await flushUi();

    expect(sidebar.getAttribute("data-mobile-open")).toBeNull();
    expect(trigger.getAttribute("aria-expanded")).toBe("false");
  });
});
