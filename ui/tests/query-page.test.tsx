import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";

import RootLayout from "@/pages/_layout";
import AppLayout from "@/pages/app/_layout";
import QueryPage from "@/pages/app/query";
import { type ColumnMeta, type QueryExplainResponse, type QuerySchemaResponse } from "@/adapters";
import { fetchMock, mockJsonResponse, resetFetchMock } from "./support/mock-fetch";
import { saveQueryWorkspace } from "@/features/query/query-tabs";
import { queryService } from "@/features/query/query-service";

const schemaResponse: QuerySchemaResponse = {
  sections: [
    {
      id: "tables",
      label: "Tables",
      items: [
        {
          id: "table:postgres.public.documents",
          kind: "table",
          label: "postgres.public.documents",
          database: "postgres",
          schema: "public",
          name: "documents",
          columns: [],
          metadata: "2 columns",
        },
        {
          id: "table:postgres.public.accounts",
          kind: "table",
          label: "postgres.public.accounts",
          database: "postgres",
          schema: "public",
          name: "accounts",
          columns: [],
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
  mockJsonResponse("/api/v1/admin/catalog", schemaResponse);
}

function mockQuerySchemaWithColumnsSuccess() {
  const data: QuerySchemaResponse = {
    sections: [
      {
        id: "tables",
        label: "Tables",
        items: [
          {
            id: "table:postgres.public.documents",
            kind: "table",
            label: "postgres.public.documents",
            database: "postgres",
            schema: "public",
            name: "documents",
            metadata: "2 columns",
            columns: [
              {
                id: "column:postgres.public.documents:id",
                name: "id",
                data_type: "uuid",
                primary_key: true,
              },
              {
                id: "column:postgres.public.documents:title",
                name: "title",
                data_type: "text",
                primary_key: false,
              },
            ],
          },
        ],
      },
      { id: "views", label: "Views", items: [] },
      { id: "indexes", label: "Indexes", items: [] },
      { id: "udfs", label: "UDFs", items: [] },
      { id: "procedures", label: "Procedures", items: [] },
    ],
  };

  mockJsonResponse("/api/v1/admin/catalog", data);
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

const explainPlan = {
  format_version: 1,
  summary: {
    collection: "postgres.public.documents",
    root_operator: "Select",
    access_path: "index_seek",
    selected_index: "postgres.public.idx_documents_title",
    selected_cost: 4,
    estimated_rows: 1,
    storage_mode: "row",
  },
  nodes: [
    {
      id: "read",
      label: "Read with idx_documents_title",
      kind: "read",
      detail: "postgres.public.documents via index_seek",
      status: "optimized",
      badges: ["index:idx_documents_title", "predicate pushdown"],
      metrics: [
        { label: "estimated rows", value: "1" },
        { label: "selected cost", value: "4" },
      ],
    },
    {
      id: "project",
      label: "Project rows",
      kind: "project",
      detail: "narrow",
      status: "active",
      badges: ["field:title"],
      metrics: [{ label: "scan fields", value: "title" }],
    },
  ],
  attributes: [
    { label: "Access path", value: "index_seek", intent: "success" },
    { label: "Index", value: "idx_documents_title", intent: "success" },
    { label: "Projection", value: "narrow", intent: "neutral" },
  ],
  estimates: {
    scan_rows: 230,
    index_rows: 1,
    join_rows: 0,
    search_rows: 0,
    vector_rows: 0,
    aggregate_rows: 0,
    scan_cost: 230,
    index_cost: 4,
    selected_cost: 4,
    cost_source: "planner",
    rejected_alternatives: ["full_scan"],
  },
  features: [
    {
      id: "predicate_pushdown",
      label: "Predicate pushdown",
      enabled: true,
      intent: "success",
      detail: "Filters applied before rows leave storage",
      node_id: "read",
    },
    {
      id: "projection_pruning",
      label: "Projection pruning",
      enabled: true,
      intent: "success",
      detail: "Read path narrows scanned fields when possible",
      node_id: "read",
    },
    {
      id: "top_k",
      label: "Top K",
      enabled: false,
      intent: "neutral",
      detail: "Ordering and limit can stop early",
      node_id: "top_k",
    },
  ],
  diagnostics: {
    access_path_reason: "scalar-index-seek",
    fallback_reason: "none",
    pagination_strategy: "none",
    early_stop: "none",
    projection_shape: "narrow",
    operator_feedback_state: "none",
    operator_feedback_reason: "none",
    adaptive_enabled: false,
    adaptive_decision_point: "none",
    adaptive_candidates: [],
    adaptive_selected_alternative: "none",
    adaptive_reason: "none",
    join_strategy: "none",
    join_fallback_reason: "none",
    rollup_rewrite: "none",
    projection_freshness: "current",
  },
} satisfies QueryExplainResponse["plan"];

function mockValidateSuccess() {
  mockJsonResponse(
    "/api/v1/admin/query-validations",
    {
      columns: [column("id"), column("name")],
      command: "SELECT",
      valid: true,
    },
    { method: "POST" },
  );
}

function mockValidateError() {
  mockJsonResponse(
    "/api/v1/admin/query-validations",
    { error: 'syntax error at or near "SELET"' },
    { method: "POST", status: 400 },
  );
}

function mockExecuteSuccess() {
  mockJsonResponse(
    "/api/v1/admin/query-executions",
    {
      columns: [column("id"), column("name")],
      command: "SELECT",
      rows: [["doc-1", "Document 1"]],
    },
    { method: "POST" },
  );
}

function mockSchemaChangingCommandSuccess(command = "CREATE TABLE") {
  mockJsonResponse(
    "/api/v1/admin/query-executions",
    {
      columns: [],
      command,
      rows: [],
    },
    { method: "POST" },
  );
}

function mockExecuteWithNullSuccess() {
  mockJsonResponse(
    "/api/v1/admin/query-executions",
    {
      columns: [column("id"), column("name")],
      command: "SELECT",
      rows: [
        ["doc-1", null],
        ["doc-2", "NULL"],
      ],
    },
    { method: "POST" },
  );
}

function mockExecuteWithTypedValuesSuccess() {
  mockJsonResponse(
    "/api/v1/admin/query-executions",
    {
      columns: [
        column("count"),
        column("active"),
        column("profile"),
        column("tags"),
        column("missing"),
      ],
      command: "SELECT",
      rows: [[42, true, { name: "Ada" }, ["sql", 2], null]],
    },
    { method: "POST" },
  );
}

function mockExecuteError() {
  mockJsonResponse(
    "/api/v1/admin/query-executions",
    { error: "collection not found: missing_table" },
    { method: "POST", status: 404 },
  );
}

function mockExplainError() {
  mockJsonResponse(
    "/api/v1/admin/query-explanations",
    { error: "query timeout exceeded" },
    { method: "POST", status: 504 },
  );
}

function mockExplainSuccess() {
  mockJsonResponse(
    "/api/v1/admin/query-explanations",
    {
      columns: [column("QUERY PLAN")],
      command: "EXPLAIN",
      rows: [["Index Scan using idx_documents_title on documents\n  Index Cond: (title = 'one')"]],
      plan: explainPlan,
    },
    { method: "POST" },
  );
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
  resetFetchMock();
});

beforeEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
  mockQuerySchemaSuccess();
  queryService.invalidateSchema("postgres");
  mockJsonResponse("/api/v1/admin/databases", [{ name: "postgres" }]);
  saveQueryWorkspace("anonymous", {
    version: 1,
    activeTabId: "query-1",
    tabs: [
      {
        id: "query-1",
        ordinal: 1,
        title: "Query 1",
        database: "postgres",
        sql: "SELECT 1 AS ready;",
      },
    ],
  });
});

describe("admin query page composition", () => {
  it("should_refresh_the_schema_given_a_successful_create_table", async () => {
    // Arrange
    mockSchemaChangingCommandSuccess();
    const root = await mountQueryRoute();
    updateEditor(root, "CREATE TABLE ui_demo (demo_id INT PRIMARY KEY, name TEXT NOT NULL);");
    await flushUi();

    // Act
    buttonByText(root, "Run").click();
    await waitForText(root, "CREATE TABLE");

    // Assert
    const catalogRequests = fetchMock.mock.calls.filter(
      ([request]) => new URL(request.url).pathname === "/api/v1/admin/catalog",
    );
    expect(catalogRequests).toHaveLength(2);
  });

  it("should_refresh_the_schema_given_a_successful_graph_ddl_command", async () => {
    // Arrange
    mockSchemaChangingCommandSuccess("CREATE GRAPH");
    const root = await mountQueryRoute();
    updateEditor(root, "CREATE GRAPH ui_graph;");
    await flushUi();

    // Act
    buttonByText(root, "Run").click();
    await waitForText(root, "CREATE GRAPH");

    // Assert
    const catalogRequests = fetchMock.mock.calls.filter(
      ([request]) => new URL(request.url).pathname === "/api/v1/admin/catalog",
    );
    expect(catalogRequests).toHaveLength(2);
  });

  it("should_keep_workspace_chrome_compact_given_the_query_page", async () => {
    // Arrange
    const root = await mountQueryRoute();

    // Act
    const heading = root.querySelector("#query-workspace-title-query-1");

    // Assert
    expect(heading?.textContent).toBe("Query workspace");
    expect(heading?.classList.contains("sr-only")).toBe(true);
    expect(root.querySelector('[data-slot="page-header"]')).toBe(null);
    expect(root.querySelector('[data-testid="query-starters"]')).toBe(null);
    expect(root.querySelector("[data-query-page]")?.getAttribute("aria-labelledby")).toBe(
      "query-tab-query-1",
    );
  });

  it("renders shell structure and query page containers", async () => {
    const root = await mountQueryRoute();

    expect(root.querySelector('[data-testid="cassie-admin-shell"]')).toBeTruthy();
    const queryPage = root.querySelector("[data-query-page]");
    expect(queryPage).toBeTruthy();
    expect(root.querySelectorAll("#main-content")).toHaveLength(1);
    expect(queryPage?.id).toBe("query-workspace-query-1");
    expect(queryPage?.getAttribute("role")).toBe("tabpanel");
    expect(queryPage?.getAttribute("aria-labelledby")).toBe("query-tab-query-1");
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

  it("moves the active tab indicator when a different result tab is clicked", async () => {
    const root = await mountQueryRoute();

    const gridTab = root.querySelector('[data-testid="query-result-tab-results"]');
    const listTab = root.querySelector('[data-testid="query-result-tab-list"]');
    if (!(gridTab instanceof HTMLElement) || !(listTab instanceof HTMLElement)) {
      throw new Error("Missing result tabs");
    }

    expect(gridTab.getAttribute("data-active")).toBe("true");
    expect(listTab.getAttribute("data-active")).toBe(null);

    listTab.click();
    await flushUi();

    expect(gridTab.getAttribute("data-active")).toBe(null);
    expect(listTab.getAttribute("data-active")).toBe("true");
    expect(root.querySelector('[data-tab-content="list"]')).toBeTruthy();
  });

  it("updates query text on schema item selection", async () => {
    const root = await mountQueryRoute();

    const schemaItem = root.querySelector('[data-item-id="table:postgres.public.documents"]');
    if (!schemaItem) {
      throw new Error("Missing schema item");
    }

    const editor = editorTextarea(root);

    expect(schemaItem.getAttribute("data-item-kind")).toBe("table");
    expect(editor.value).toBe("SELECT 1 AS ready;");
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

  it("shows a danger toast when validation itself fails, instead of failing silently", async () => {
    mockValidateError();
    const root = await mountQueryRoute();

    buttonByText(root, "Validate").click();
    await waitForText(root, "Validation failed");

    const toast = root.querySelector('[data-testid="query-validation-toast"]');
    if (!(toast instanceof HTMLElement)) {
      throw new Error("Missing validation toast");
    }
    expect(toast.hidden).toBe(false);
    expect(toast.getAttribute("data-variant")).toBe("danger");
    expect(toast.textContent).toContain('syntax error at or near "SELET"');
  });

  it("shows validation results as a dismissible toast, not a persistent banner", async () => {
    mockValidateSuccess();
    const root = await mountQueryRoute();

    buttonByText(root, "Validate").click();
    await waitForText(root, "Validation passed");

    const toast = root.querySelector('[data-testid="query-validation-toast"]');
    if (!(toast instanceof HTMLElement)) {
      throw new Error("Missing validation toast");
    }
    expect(toast.hidden).toBe(false);
    expect(toast.getAttribute("data-variant")).toBe("success");

    const dismissButton = toast.querySelector('button[aria-label="Dismiss notification"]');
    if (!(dismissButton instanceof HTMLElement)) {
      throw new Error("Missing toast dismiss button");
    }
    dismissButton.click();
    await flushUi();
    expect(toast.hidden).toBe(true);
  });

  it("should_expose_keyboard_resizing_for_split_handles", async () => {
    const root = await mountQueryRoute();
    const handle = root.querySelector(
      '[data-testid="query-resizable-split-vertical"] > [role="separator"]',
    );
    if (!(handle instanceof HTMLElement)) {
      throw new Error("Missing vertical split handle");
    }

    expect(handle.getAttribute("aria-orientation")).toBe("horizontal");
    expect(handle.getAttribute("aria-valuemin")).toBe("30");
    expect(handle.getAttribute("aria-valuemax")).toBe("80");
    expect(handle.getAttribute("aria-valuenow")).toBe("52");

    handle.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, key: "ArrowDown" }));
    await flushUi();

    expect(handle.getAttribute("aria-valuenow")).toBe("54");
  });

  it("resizes the vertical split via pointer drag inside the full query page", async () => {
    const root = await mountQueryRoute();
    const container = root.querySelector('[data-testid="query-resizable-split-vertical"]');
    const handle = container?.querySelector('[role="separator"]');
    const pane = container?.querySelector(".cassie-resizable-split-pane");
    if (
      !(container instanceof HTMLElement) ||
      !(handle instanceof HTMLElement) ||
      !(pane instanceof HTMLElement)
    ) {
      throw new Error("Missing vertical split container, handle, or pane");
    }

    container.getBoundingClientRect = () =>
      ({
        top: 0,
        left: 0,
        width: 400,
        height: 400,
        right: 400,
        bottom: 400,
        x: 0,
        y: 0,
        toJSON() {
          return {};
        },
      }) as DOMRect;
    handle.setPointerCapture = () => {};
    handle.releasePointerCapture = () => {};

    expect(handle.getAttribute("aria-valuenow")).toBe("52");

    handle.dispatchEvent(
      new PointerEvent("pointerdown", { bubbles: true, clientX: 100, clientY: 248, pointerId: 1 }),
    );
    await flushUi();

    handle.dispatchEvent(
      new PointerEvent("pointermove", { bubbles: true, clientX: 100, clientY: 300, pointerId: 1 }),
    );
    await flushUi();

    expect(handle.getAttribute("aria-valuenow")).toBe("75");
    expect(pane.style.blockSize).toBe("75%");

    handle.dispatchEvent(
      new PointerEvent("pointerup", { bubbles: true, clientX: 100, clientY: 300, pointerId: 1 }),
    );
    await flushUi();

    expect(handle.getAttribute("aria-valuenow")).toBe("75");
    expect(pane.style.blockSize).toBe("75%");
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
    expect(
      tablesSection.querySelector('[data-item-id="table:postgres.public.documents"]'),
    ).toBeTruthy();

    tablesToggle.click();
    await flushUi();
    expect(tablesToggle.getAttribute("aria-expanded")).toBe("false");
    expect(tablesSection.querySelector('[data-item-id="table:postgres.public.documents"]')).toBe(
      null,
    );

    tablesToggle.click();
    await flushUi();
    expect(tablesToggle.getAttribute("aria-expanded")).toBe("true");
    expect(
      tablesSection.querySelector('[data-item-id="table:postgres.public.documents"]'),
    ).toBeTruthy();
  });

  it("selects a schema item without overwriting the SQL editor", async () => {
    const root = await mountQueryRoute();
    const editor = editorTextarea(root);
    const originalValue = editor.value;

    const item = root.querySelector('[data-item-id="table:postgres.public.documents"]');
    if (!(item instanceof HTMLElement)) {
      throw new Error("Missing schema item");
    }

    item.click();
    await flushUi();

    expect(editor.value).toBe(originalValue);
    expect(item.getAttribute("aria-current")).toBe("true");
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

  it("runs the query when Ctrl/Cmd+Enter is pressed in the SQL editor", async () => {
    mockExecuteSuccess();
    const root = await mountQueryRoute();
    const editor = editorTextarea(root);

    const event = new KeyboardEvent("keydown", {
      key: "Enter",
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    editor.dispatchEvent(event);
    await waitForText(root, "1 row");

    expect(event.defaultPrevented).toBe(true);
    expect(root.textContent).toContain("1 row");
  });

  it("nests schema sections under a postgres database and public namespace, both expanded by default", async () => {
    const root = await mountQueryRoute();

    const database = root.querySelector(
      '[data-testid="query-schema-tree-database"][data-database="postgres"]',
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
    expect(
      namespace.querySelector('[data-item-id="table:postgres.public.documents"]'),
    ).toBeTruthy();

    namespaceToggle.click();
    await flushUi();
    expect(namespaceToggle.getAttribute("aria-expanded")).toBe("false");
    expect(namespace.querySelector('[data-item-id="table:postgres.public.documents"]')).toBe(null);

    namespaceToggle.click();
    await flushUi();
    expect(namespaceToggle.getAttribute("aria-expanded")).toBe("true");
    expect(
      namespace.querySelector('[data-item-id="table:postgres.public.documents"]'),
    ).toBeTruthy();
  });

  it("renders a kind icon on schema items", async () => {
    const root = await mountQueryRoute();

    const item = root.querySelector('[data-item-id="table:postgres.public.documents"]');
    expect(item?.querySelector("svg")).toBeTruthy();
  });

  it("expands a table to show its columns, with a key icon on the primary key", async () => {
    mockQuerySchemaWithColumnsSuccess();
    const root = await mountQueryRoute();

    const item = root.querySelector('[data-item-id="table:postgres.public.documents"]');
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

  it("renders the explain plan as a visual plan with raw text, not JSON", async () => {
    mockExplainSuccess();
    const root = await mountQueryRoute();

    buttonByText(root, "Explain").click();
    await waitForText(root, "Read with idx_documents_title");

    const planPanel = root.querySelector('[data-tab-content="plan"]');
    expect(planPanel?.querySelector('[data-testid="query-plan-visual"]')).toBeTruthy();
    expect(planPanel?.querySelectorAll('[data-testid="query-plan-node"]').length).toBe(2);
    expect(planPanel?.textContent).toContain("Predicate pushdown");
    expect(planPanel?.textContent).toContain("scalar-index-seek");
    expect(planPanel?.querySelector("pre.cassie-query-plan-text")).toBeTruthy();
    expect(planPanel?.textContent).toContain("Index Scan using idx_documents_title");
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

  it("should_preserve_wire_types_in_the_json_results_view", async () => {
    // Arrange
    mockExecuteWithTypedValuesSuccess();
    const root = await mountQueryRoute();

    // Act
    buttonByText(root, "Run").click();
    await waitForText(root, "1 row");
    buttonByText(root, "JSON").click();
    await flushUi();

    // Assert
    const json = root.querySelector(".cassie-query-json code")?.textContent;
    expect(json).toBeTruthy();
    expect(JSON.parse(json ?? "{}").rows[0]).toEqual([42, true, { name: "Ada" }, ["sql", 2], null]);
  });

  it("should_render_execute_failures_without_an_unhandled_rejection", async () => {
    // Arrange
    mockExecuteError();
    const root = await mountQueryRoute();

    // Act
    buttonByText(root, "Run").click();
    await waitForText(root, "collection not found: missing_table");

    // Assert
    expect(root.textContent).toContain("Query action failed");
    expect(buttonByText(root, "Run").disabled).toBe(false);
  });

  it("should_render_explain_failures_without_an_unhandled_rejection", async () => {
    // Arrange
    mockExplainError();
    const root = await mountQueryRoute();

    // Act
    buttonByText(root, "Explain").click();
    await waitForText(root, "query timeout exceeded");

    // Assert
    expect(root.textContent).toContain("Query action failed");
    expect(buttonByText(root, "Explain").disabled).toBe(false);
  });

  it("should_hide_a_previous_execute_error_given_a_successful_explain", async () => {
    // Arrange
    mockExecuteError();
    mockExplainSuccess();
    const root = await mountQueryRoute();
    buttonByText(root, "Run").click();
    await waitForText(root, "collection not found: missing_table");

    // Act
    buttonByText(root, "Explain").click();
    await waitForText(root, "Read with idx_documents_title");

    // Assert
    expect(root.textContent).not.toContain("collection not found: missing_table");
  });

  it("should_hide_a_previous_explain_error_given_a_successful_execute", async () => {
    // Arrange
    mockExplainError();
    mockExecuteSuccess();
    const root = await mountQueryRoute();
    buttonByText(root, "Explain").click();
    await waitForText(root, "query timeout exceeded");

    // Act
    buttonByText(root, "Run").click();
    await waitForText(root, "1 row");

    // Assert
    expect(root.textContent).not.toContain("query timeout exceeded");
  });
});
