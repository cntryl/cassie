import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";

import RootLayout from "@/pages/_layout";
import AppLayout from "@/pages/app/_layout";
import QueryPage from "@/pages/app/query";
import { querySchema } from "@/data/query-schema";

async function mountQueryRoute() {
  cleanupApp("app");
  document.body.innerHTML = '<div id="app"></div>';
  window.history.pushState({}, "", "/admin/query");

  const root = document.getElementById("app");
  if (!root) {
    throw new Error("Missing test app root");
  }

  await createSPA({
    root,
    routes: [
      {
        path: "/admin/query",
        handler: () => (
          <RootLayout>
            <AppLayout>
              <QueryPage />
            </AppLayout>
          </RootLayout>
        ),
      },
    ],
  });

  await new Promise<void>((resolve) => queueMicrotask(() => resolve()));
  return root;
}

afterEach(() => {
  cleanupApp("app");
  document.body.innerHTML = "";
});

describe("admin query page composition", () => {
  it("should_render_shell_and_query_page_composition", async () => {
    const root = await mountQueryRoute();

    expect(root.querySelector('[data-testid="cassie-admin-shell"]')).toBeTruthy();
    const queryPage = root.querySelector("[data-query-page]");
    expect(queryPage).toBeTruthy();
    expect(queryPage?.id).toBe("main-content");
    expect(queryPage?.getAttribute("tabindex")).toBe("-1");
    expect(queryPage?.getAttribute("aria-labelledby")).toBe("cassie-admin-page-title");
    expect(root.querySelector('[data-testid="query-schema-tree"]')).toBeTruthy();
    expect(root.querySelector('[data-testid="query-editor-panel"]')).toBeTruthy();
    expect(root.querySelector('[data-testid="query-editor-toolbar"]')).toBeTruthy();
    expect(root.querySelector('[data-testid="query-results-tabs"]')).toBeTruthy();
  });

  it("should_switch_result_tabs_and_emit_tab_callback", async () => {
    const onActiveTabChange = vi.fn();
    cleanupApp("app");
    document.body.innerHTML = '<div id="app"></div>';
    window.history.pushState({}, "", "/admin/query");

    const root = document.getElementById("app");
    if (!root) {
      throw new Error("Missing test app root");
    }

    await createSPA({
      root,
      routes: [
        {
          path: "/admin/query",
          handler: () => (
            <RootLayout>
              <AppLayout>
                <QueryPage onActiveTabChange={onActiveTabChange} />
              </AppLayout>
            </RootLayout>
          ),
        },
      ],
    });

    await new Promise<void>((resolve) => queueMicrotask(() => resolve()));

    const listTab = root.querySelector('[data-testid="query-result-tab-list"]');
    const planTab = root.querySelector('[data-testid="query-result-tab-plan"]');
    if (!listTab || !planTab) {
      throw new Error("Missing result tabs");
    }

    listTab.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(root.querySelector('[data-tab-content="list"]')).toBeTruthy();
    expect(onActiveTabChange).toHaveBeenCalledWith("list");

    planTab.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(root.querySelector('[data-tab-content="plan"]')).toBeTruthy();
    expect(onActiveTabChange).toHaveBeenCalledWith("plan");
  });

  it("should_emit_schema_selection_and_mount_query_editor", async () => {
    const onSchemaItemSelect = vi.fn();
    cleanupApp("app");
    document.body.innerHTML = '<div id="app"></div>';
    window.history.pushState({}, "", "/admin/query");

    const root = document.getElementById("app");
    if (!root) {
      throw new Error("Missing test app root");
    }

    await createSPA({
      root,
      routes: [
        {
          path: "/admin/query",
          handler: () => (
            <RootLayout>
              <AppLayout>
                <QueryPage onSchemaItemSelect={onSchemaItemSelect} />
              </AppLayout>
            </RootLayout>
          ),
        },
      ],
    });

    await new Promise<void>((resolve) => queueMicrotask(() => resolve()));

    const firstItem = querySchema[0]?.items[0];
    if (!firstItem) {
      throw new Error("Expected a schema fixture item");
    }

    const renderedItem = root.querySelector(`[data-item-id="${firstItem.id}"]`);
    if (!renderedItem) {
      throw new Error("Could not find rendered schema item");
    }

    renderedItem.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    expect(onSchemaItemSelect).toHaveBeenCalledWith(firstItem);

    const secondRoot = document.querySelector("[data-query-page]");
    if (!secondRoot) {
      throw new Error("Missing query page for rerender");
    }

    const editor = secondRoot.querySelector('[data-query-editor="fallback"] textarea');
    expect(editor).toBeTruthy();
  });

  it("should_mount_the_sql_editor_host_with_expected_attributes", async () => {
    const root = await mountQueryRoute();

    const editorHost = root.querySelector('[data-query-editor="fallback"]');
    expect(editorHost).toBeTruthy();
    expect(editorHost?.getAttribute("aria-label")).toBe("SQL editor");
  });
});
