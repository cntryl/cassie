import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";
import type { FetchResponse } from "@fgrzl/fetch";

import RootLayout from "@/pages/_layout";
import AppLayout from "@/pages/app/_layout";
import QueryPage from "@/pages/app/query";
import { apiv1, type QuerySchemaResponse } from "@/adapters";

const schemaResponse: QuerySchemaResponse = {
  sections: [
    {
      id: "tables",
      label: "Tables",
      items: [
        {
          id: "table:documents",
          kind: "table",
          label: "documents",
          metadata: "2 columns",
        },
      ],
    },
    {
      id: "views",
      label: "Views",
      items: [],
    },
    {
      id: "indexes",
      label: "Indexes",
      items: [],
    },
    {
      id: "udfs",
      label: "UDFs",
      items: [],
    },
    {
      id: "procedures",
      label: "Procedures",
      items: [],
    },
  ],
};

function mockQuerySchemaSuccess() {
  const response: FetchResponse<QuerySchemaResponse> = {
    ok: true,
    status: 200,
    statusText: "OK",
    headers: new Headers(),
    url: "/v1/admin/query/schema",
    data: schemaResponse,
    error: null,
  };

  vi.spyOn(apiv1, "getAdminQuerySchema").mockResolvedValue(response);
}

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
  vi.clearAllMocks();
  cleanupApp("app");
  document.body.innerHTML = "";
});

beforeEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
  mockQuerySchemaSuccess();
});

describe("admin query page composition", () => {
  it("renders shell structure and query page containers", async () => {
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

  it("renders result tabs and default content", async () => {
    const root = await mountQueryRoute();

    const listTab = root.querySelector('[data-testid="query-result-tab-list"]');
    const planTab = root.querySelector('[data-testid="query-result-tab-plan"]');
    if (!listTab || !planTab) {
      throw new Error("Missing result tabs");
    }

    expect(listTab.textContent).toBeTruthy();
    expect(planTab.textContent).toBeTruthy();
    expect(root.querySelector('[data-tab-content="results"]')).toBeTruthy();
  });

  it("updates query text on schema item selection", async () => {
    const root = await mountQueryRoute();

    const schemaItem = root.querySelector('[data-item-id="table:documents"]');
    if (!schemaItem) {
      throw new Error("Missing schema item");
    }

    const editor = root.querySelector('[data-query-editor="fallback"] textarea') as HTMLTextAreaElement | null;
    if (!editor) {
      throw new Error("Missing fallback editor");
    }

    expect(schemaItem.getAttribute("data-item-kind")).toBe("table");
    expect(editor.value).toContain("SELECT id, name");
  });
});
