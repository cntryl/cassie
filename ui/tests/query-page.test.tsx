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
        {
          id: "table:accounts",
          kind: "table",
          label: "accounts",
          metadata: "6 columns",
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
    url: "/api/v1/admin/query/schema",
    data: schemaResponse,
    error: null,
  };

  vi.spyOn(apiv1, "getAdminQuerySchema").mockResolvedValue(response);
}

function mockQuerySchemaWithColumnsSuccess() {
  // Columns aren't part of the generated QuerySchemaResponse type yet (see
  // query-mappers.ts's forward-compat comment) — cast the raw fixture.
  const data = {
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
            columns: [
              { id: "documents:id", name: "id", dataType: "uuid", primaryKey: true },
              { id: "documents:title", name: "title", dataType: "text" },
            ],
          },
        ],
      },
      { id: "views", label: "Views", items: [] },
      { id: "indexes", label: "Indexes", items: [] },
      { id: "udfs", label: "UDFs", items: [] },
      { id: "procedures", label: "Procedures", items: [] },
    ],
  } as unknown as QuerySchemaResponse;

  const response: FetchResponse<QuerySchemaResponse> = {
    ok: true,
    status: 200,
    statusText: "OK",
    headers: new Headers(),
    url: "/api/v1/admin/query/schema",
    data,
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
    url: "/api/v1/admin/query/validate",
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
    url: "/api/v1/admin/query/execute",
    data: {
      columns: [column("id"), column("name")],
      command: "SELECT",
      rows: [["doc-1", "Document 1"]],
    },
    error: null,
  };

  vi.spyOn(apiv1, "executeAdminQuery").mockResolvedValue(response);
}

function mockExecuteWithNullSuccess() {
  const response: FetchResponse<QueryResult> = {
    ok: true,
    status: 200,
    statusText: "OK",
    headers: new Headers(),
    url: "/api/v1/admin/query/execute",
    data: {
      columns: [column("id"), column("name")],
      command: "SELECT",
      rows: [
        ["doc-1", null],
        ["doc-2", "NULL"],
      ],
    },
    error: null,
  };

  vi.spyOn(apiv1, "executeAdminQuery").mockResolvedValue(response);
}

function mockExplainSuccess() {
  const response: FetchResponse<QueryResult> = {
    ok: true,
    status: 200,
    statusText: "OK",
    headers: new Headers(),
    url: "/api/v1/admin/query/explain",
    data: {
      columns: [column("QUERY PLAN")],
      command: "EXPLAIN",
      rows: [["Seq Scan on documents\n  Filter: (id > 0)"]],
    },
    error: null,
  };

  vi.spyOn(apiv1, "explainAdminQuery").mockResolvedValue(response);
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
  window.history.pushState({}, "", "/");

  const root = document.getElementById("app");
  if (!root) {
    throw new Error("Missing test app root");
  }

  await createSPA({
    root,
    routes: [
      {
        path: "/",
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
    const schemaBrowser = root.querySelector('[aria-label="Schema browser"]');
    const schemaTree = root.querySelector('[data-testid="query-schema-tree"]');
    expect(schemaTree).toBeTruthy();
    expect(schemaBrowser?.contains(schemaTree)).toBe(true);
    expect(root.querySelector(".cassie-query-workspace [data-testid='query-schema-tree']")).toBe(
      null,
    );
    expect(root.querySelector('[data-testid="query-resizable-split-horizontal"]')).toBe(null);
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
      '[data-testid="query-resizable-split-vertical"] > [role="separator"]',
    );
    if (!(handle instanceof HTMLElement)) {
      throw new Error("Missing vertical split handle");
    }

    expect(handle.getAttribute("aria-orientation")).toBe("vertical");
    expect(handle.getAttribute("aria-valuemin")).toBe("30");
    expect(handle.getAttribute("aria-valuemax")).toBe("80");
    expect(handle.getAttribute("aria-valuenow")).toBe("62");

    handle.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "ArrowDown" }));
    await flushUi();

    expect(handle.getAttribute("aria-valuenow")).toBe("64");
  });

  it("collapses and expands schema sections, defaulting empty sections to collapsed", async () => {
    const root = await mountQueryRoute();

    const tablesSection = root.querySelector(
      '[data-testid="query-schema-tree-section"][data-section="tables"]',
    );
    const viewsSection = root.querySelector(
      '[data-testid="query-schema-tree-section"][data-section="views"]',
    );
    if (!tablesSection || !viewsSection) {
      throw new Error("Missing schema sections");
    }

    const tablesToggle = tablesSection.querySelector("[aria-expanded]");
    const viewsToggle = viewsSection.querySelector("[aria-expanded]");
    if (!(tablesToggle instanceof HTMLElement) || !(viewsToggle instanceof HTMLElement)) {
      throw new Error("Missing section toggles");
    }

    expect(tablesToggle.getAttribute("aria-expanded")).toBe("true");
    expect(viewsToggle.getAttribute("aria-expanded")).toBe("false");
    expect(tablesSection.querySelector('[data-item-id="table:documents"]')).toBeTruthy();

    tablesToggle.click();
    await flushUi();
    expect(tablesToggle.getAttribute("aria-expanded")).toBe("false");
    expect(tablesSection.querySelector('[data-item-id="table:documents"]')).toBe(null);

    tablesToggle.click();
    await flushUi();
    expect(tablesToggle.getAttribute("aria-expanded")).toBe("true");
    expect(tablesSection.querySelector('[data-item-id="table:documents"]')).toBeTruthy();
  });

  it("selects a schema item without overwriting the SQL editor", async () => {
    const root = await mountQueryRoute();
    const editor = editorTextarea(root);
    const originalValue = editor.value;

    const item = root.querySelector('[data-item-id="table:documents"]');
    if (!(item instanceof HTMLElement)) {
      throw new Error("Missing schema item");
    }

    item.click();
    await flushUi();

    expect(editor.value).toBe(originalValue);
    expect(item.getAttribute("aria-pressed")).toBe("true");
  });

  it("inserts a soft tab instead of moving focus when Tab is pressed in the SQL editor", async () => {
    const root = await mountQueryRoute();
    const editor = editorTextarea(root);

    editor.selectionStart = editor.value.length;
    editor.selectionEnd = editor.value.length;
    const originalValue = editor.value;

    const event = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
    editor.dispatchEvent(event);
    await flushUi();
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

    expect(event.defaultPrevented).toBe(true);
    expect(editor.value).toBe(`${originalValue}  `);
  });

  it("nests schema sections under a default database and namespace, both expanded by default", async () => {
    const root = await mountQueryRoute();

    const database = root.querySelector(
      '[data-testid="query-schema-tree-database"][data-database="default"]',
    );
    const namespace = root.querySelector(
      '[data-testid="query-schema-tree-namespace"][data-namespace="public"]',
    );
    if (!database || !namespace) {
      throw new Error("Missing database/namespace tree levels");
    }

    const databaseToggle = database.querySelector("[aria-expanded]");
    const namespaceToggle = namespace.querySelector("[aria-expanded]");
    if (!(databaseToggle instanceof HTMLElement) || !(namespaceToggle instanceof HTMLElement)) {
      throw new Error("Missing database/namespace toggles");
    }

    expect(databaseToggle.getAttribute("aria-expanded")).toBe("true");
    expect(namespaceToggle.getAttribute("aria-expanded")).toBe("true");
    expect(
      database.querySelector(
        '[data-testid="query-schema-tree-namespace"][data-namespace="public"]',
      ),
    ).toBeTruthy();
    expect(namespace.querySelector('[data-item-id="table:documents"]')).toBeTruthy();

    namespaceToggle.click();
    await flushUi();
    expect(namespaceToggle.getAttribute("aria-expanded")).toBe("false");
    expect(namespace.querySelector('[data-item-id="table:documents"]')).toBe(null);

    namespaceToggle.click();
    await flushUi();
    expect(namespaceToggle.getAttribute("aria-expanded")).toBe("true");
    expect(namespace.querySelector('[data-item-id="table:documents"]')).toBeTruthy();
  });

  it("renders a kind icon on schema items", async () => {
    const root = await mountQueryRoute();

    const item = root.querySelector('[data-item-id="table:documents"]');
    expect(item?.querySelector("svg")).toBeTruthy();
  });

  it("expands a table to show its columns, with a key icon on the primary key", async () => {
    mockQuerySchemaWithColumnsSuccess();
    const root = await mountQueryRoute();

    const item = root.querySelector('[data-item-id="table:documents"]');
    const row = item?.closest('[data-testid="query-schema-item-row"]');
    if (!row) {
      throw new Error("Missing schema item row");
    }

    const menuItem = row.parentElement;
    const columnsList = menuItem?.querySelector('[data-testid="query-schema-item-columns"]');
    if (!(columnsList instanceof HTMLElement)) {
      throw new Error("Missing columns list");
    }

    expect(row.getAttribute("data-expandable")).toBe("true");
    expect(columnsList.hidden).toBe(true);

    const toggle = row.querySelector('[data-testid="query-schema-item-toggle"]');
    if (!(toggle instanceof HTMLElement)) {
      throw new Error("Missing column toggle");
    }

    toggle.click();
    await flushUi();
    expect(columnsList.hidden).toBe(false);

    const columns = columnsList.querySelectorAll('[data-testid="query-schema-column"]');
    expect(columns.length).toBe(2);
    expect(columns[0].getAttribute("data-primary-key")).toBe("true");
    expect(columns[0].querySelector("svg")).toBeTruthy();
    expect(columns[1].getAttribute("data-primary-key")).toBe(null);

    toggle.click();
    await flushUi();
    expect(columnsList.hidden).toBe(true);
  });

  it("renders the explain plan as formatted text, not JSON", async () => {
    mockExplainSuccess();
    const root = await mountQueryRoute();

    buttonByText(root, "Explain").click();
    await waitForText(root, "Seq Scan on documents");

    const planPanel = root.querySelector('[data-tab-content="plan"]');
    expect(planPanel?.querySelector("pre.cassie-query-plan-text")).toBeTruthy();
    expect(planPanel?.querySelector(".cassie-query-json")).toBe(null);
  });

  it('renders NULL values distinctly from the literal string "NULL"', async () => {
    mockExecuteWithNullSuccess();
    const root = await mountQueryRoute();

    buttonByText(root, "Run").click();
    await waitForText(root, "doc-1");

    const nullCells = root.querySelectorAll(".cassie-query-cell-null");
    expect(nullCells.length).toBe(1);
    expect(root.textContent).toContain("NULL");
  });
});
