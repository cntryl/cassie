import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";
import type { FetchResponse } from "@fgrzl/fetch";

import RootLayout from "@/pages/_layout";
import AppLayout from "@/pages/app/_layout";
import QueryPage from "@/pages/app/query";
import {
  apiv1,
  type ColumnMeta,
  type QueryResult,
  type QuerySchemaResponse,
  type QueryValidateResponse,
} from "@/adapters";

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

function column(name: string): ColumnMeta {
  return {
    atttypmod: -1,
    data_type: "text",
    format_code: 0,
    name,
    nullable: true,
    type_oid: 25,
    typlen: -1,
  };
}

function mockValidateSuccess() {
  const response: FetchResponse<QueryValidateResponse> = {
    ok: true,
    status: 200,
    statusText: "OK",
    headers: new Headers(),
    url: "/v1/admin/query/validate",
    data: {
      columns: [column("id"), column("name")],
      command: "SELECT",
      valid: true,
    },
    error: null,
  };

  vi.spyOn(apiv1, "validateAdminQuery").mockResolvedValue(response);
}

function mockExecuteSuccess() {
  const response: FetchResponse<QueryResult> = {
    ok: true,
    status: 200,
    statusText: "OK",
    headers: new Headers(),
    url: "/v1/admin/query/execute",
    data: {
      columns: [column("id"), column("name")],
      command: "SELECT",
      rows: [["doc-1", "Document 1"]],
    },
    error: null,
  };

  vi.spyOn(apiv1, "executeAdminQuery").mockResolvedValue(response);
}

async function flushUi() {
  await new Promise<void>((resolve) => queueMicrotask(() => resolve()));
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

async function waitForText(root: Element, text: string) {
  for (let attempt = 0; attempt < 10; attempt += 1) {
    await flushUi();
    if (root.textContent?.includes(text)) {
      return;
    }
  }

  throw new Error(`Timed out waiting for text ${text}. Current text: ${root.textContent ?? ""}`);
}

function buttonByText(root: Element, text: string) {
  const button = Array.from(root.querySelectorAll("button")).find((element) =>
    element.textContent?.includes(text),
  );

  if (!button) {
    throw new Error(`Missing button with text ${text}`);
  }

  return button as HTMLButtonElement;
}

function editorTextarea(root: Element) {
  const editor = root.querySelector(
    '[data-query-editor="fallback"] textarea',
  ) as HTMLTextAreaElement | null;
  if (!editor) {
    throw new Error("Missing fallback editor");
  }

  return editor;
}

function updateEditor(root: Element, value: string) {
  const editor = editorTextarea(root);
  editor.value = value;
  editor.dispatchEvent(new Event("input", { bubbles: true }));
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

  await flushUi();
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

    const editor = editorTextarea(root);

    expect(schemaItem.getAttribute("data-item-kind")).toBe("table");
    expect(editor.value).toContain("SELECT id, name");
  });

  it("should_clear_validation_and_result_feedback_when_sql_changes", async () => {
    mockValidateSuccess();
    mockExecuteSuccess();
    const root = await mountQueryRoute();

    buttonByText(root, "Validate").click();
    await waitForText(root, "Validation passed");
    expect(root.textContent).toContain("Validation passed");

    updateEditor(root, "SELECT name FROM documents;");
    await flushUi();
    expect(root.textContent).not.toContain("Validation passed");

    buttonByText(root, "Run").click();
    await waitForText(root, "1 row");
    expect(root.textContent).toContain("Command");
    expect(root.textContent).toContain("1 row");

    updateEditor(root, "SELECT id FROM documents;");
    await flushUi();
    expect(root.textContent).not.toContain("1 row");
    expect(root.textContent).toContain("No query has run yet.");
  });

  it("should_expose_keyboard_resizing_for_split_handles", async () => {
    const root = await mountQueryRoute();
    const handle = root.querySelector(
      '[data-testid="query-resizable-split-horizontal"] > [role="separator"]',
    );
    if (!(handle instanceof HTMLElement)) {
      throw new Error("Missing horizontal split handle");
    }

    expect(handle.getAttribute("aria-orientation")).toBe("horizontal");
    expect(handle.getAttribute("aria-valuemin")).toBe("18");
    expect(handle.getAttribute("aria-valuemax")).toBe("40");
    expect(handle.getAttribute("aria-valuenow")).toBe("30");

    handle.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "ArrowRight" }));
    await flushUi();

    expect(handle.getAttribute("aria-valuenow")).toBe("32");
  });
});
