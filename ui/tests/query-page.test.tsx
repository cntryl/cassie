import { describe, expect, it } from "vite-plus/test";

import { saveQueryWorkspace } from "@/features/query/query-tabs";
import {
  buttonByText,
  editorTextarea,
  fetchMock,
  flushUi,
  mockExecuteError,
  mockExecuteSuccess,
  mockExecuteWithNullSuccess,
  mockExecuteWithTypedValuesSuccess,
  mockExplainError,
  mockExplainSuccess,
  mockJsonResponse,
  mockQuerySchemaWithColumnsSuccess,
  mockValidateError,
  mockValidateSuccess,
  mountQueryRoute,
  updateEditor,
  waitForText,
} from "./support/query-page-harness";

async function expandSchemaSection(root: Element, section = "tables") {
  await waitForText(root, "public");
  const sectionElement = root.querySelector(
    `[data-testid="query-schema-tree-section"][data-section="${section}"]`,
  );
  const toggle = sectionElement?.querySelector<HTMLElement>("[aria-expanded]");
  if (!sectionElement || !toggle) {
    throw new Error(`Missing ${section} schema section`);
  }
  if (toggle.getAttribute("aria-expanded") !== "true") {
    toggle.click();
    await flushUi();
  }
  return sectionElement;
}

describe("admin query page composition", () => {
  it("should_rename_a_saved_query_inline", async () => {
    // Arrange
    const root = await mountQueryRoute();
    const renameButton = root.querySelector<HTMLButtonElement>(
      'button[aria-label="Rename Query 1"]',
    );

    // Act
    renameButton?.click();
    await flushUi();
    const input = root.querySelector<HTMLInputElement>(
      'input[aria-label="Query name for Query 1"]',
    );
    expect(input).not.toBeNull();
    if (input) {
      input.value = "Customer lookup";
      input.dispatchEvent(new InputEvent("input", { bubbles: true }));
    }
    root.querySelector<HTMLButtonElement>('button[aria-label="Save query name"]')?.click();
    await flushUi();

    // Assert
    expect(root.textContent).toContain("Customer lookup");
    expect(root.querySelector('button[aria-label="Rename Customer lookup"]')).not.toBeNull();
  });

  it("should_confirm_saved_query_deletion_with_standard_destructive_actions", async () => {
    // Arrange
    const root = await mountQueryRoute();

    // Act
    const removeButtons = Array.from(
      document.querySelectorAll<HTMLButtonElement>('button[aria-label="Delete Query 1"]'),
    );
    const removeButton = removeButtons[removeButtons.length - 1];
    expect(removeButton?.querySelector("svg")).not.toBeNull();
    removeButton?.click();
    await waitForText(document.body, "Delete query?");

    // Assert
    const dialog = document.querySelector(".cassie-delete-query-dialog");
    expect(dialog?.textContent).toContain("Delete query?");
    expect(dialog?.textContent).toContain("“Query 1” will be permanently deleted");
    expect(buttonByText(dialog ?? root, "Cancel")).not.toBeNull();
    const deleteButton = buttonByText(dialog ?? root, "Delete query");
    expect(deleteButton.getAttribute("data-variant")).toBe("destructive");
    expect(root.querySelector("#saved-query-query-1")).not.toBeNull();

    deleteButton.click();
    await flushUi();

    expect(root.querySelector("#saved-query-query-1")).toBeNull();
    expect(document.querySelector(".cassie-delete-query-dialog")).toBeNull();
  });

  it("should_keep_the_database_tree_visible_without_an_open_query", async () => {
    // Arrange
    saveQueryWorkspace("anonymous", { version: 1, activeTabId: null, tabs: [] });

    // Act
    const root = await mountQueryRoute();

    // Assert
    expect(root.querySelector('[data-testid="query-schema-tree"]')).not.toBeNull();
    expect(root.textContent).toContain("postgres");
    expect(root.querySelector('button[aria-label="Create database"]')).not.toBeNull();
  });

  it("should_show_all_databases_while_a_query_is_open", async () => {
    // Arrange
    mockJsonResponse("/api/v1/admin/databases", [{ name: "analytics" }, { name: "postgres" }]);

    // Act
    const root = await mountQueryRoute();

    // Assert
    const tree = root.querySelector('[data-testid="query-schema-tree"]');
    expect(tree?.textContent).toContain("analytics");
    expect(tree?.textContent).toContain("postgres");
  });

  it("should_create_a_database_through_the_dedicated_admin_endpoint", async () => {
    // Arrange
    mockJsonResponse(
      "/api/v1/admin/databases",
      { name: "reporting" },
      { method: "POST", status: 201 },
    );
    const root = await mountQueryRoute();
    root.querySelector<HTMLButtonElement>('button[aria-label="Create database"]')?.click();
    await flushUi();
    const dialogs = document.querySelectorAll('[role="dialog"]');
    expect(dialogs).toHaveLength(1);
    const dialog = dialogs[0];
    const nameInput = dialog?.querySelector<HTMLInputElement>("#create-database-name");
    if (!dialog || !nameInput) {
      throw new Error("Missing create database dialog");
    }

    // Act
    nameInput.value = "Reporting";
    nameInput.dispatchEvent(new Event("input", { bubbles: true }));
    await flushUi();
    const createButton = buttonByText(dialog, "Create database");
    createButton.click();
    await waitForText(root, "reporting");

    // Assert
    const createRequest = fetchMock.mock.calls
      .map(([request]) => request)
      .find(
        (request) =>
          request.method === "POST" && new URL(request.url).pathname === "/api/v1/admin/databases",
      );
    expect(createRequest).toBeDefined();
    expect(await createRequest?.clone().json()).toEqual({ name: "Reporting" });
    expect(
      fetchMock.mock.calls.some(([request]) => {
        return (
          request.method === "POST" &&
          new URL(request.url).pathname === "/api/v1/admin/query-executions"
        );
      }),
    ).toBe(false);
  });

  it("should_keep_an_unavailable_database_draft_editable", async () => {
    // Arrange
    saveQueryWorkspace("anonymous", {
      version: 1,
      activeTabId: "query-dummy",
      tabs: [
        {
          id: "query-dummy",
          ordinal: 1,
          title: "Query 1",
          database: "dummy",
          sql: "SELECT 1;",
        },
      ],
    });
    const root = await mountQueryRoute();
    const editor = editorTextarea(root);

    // Act
    editor.value = "SELECT 2;";
    editor.dispatchEvent(new Event("input", { bubbles: true }));
    await flushUi();

    // Assert
    expect(root.textContent).toContain("dummy is not on this server");
    expect(root.querySelector('[data-query-editor="fallback"] textarea')).toBe(editor);
    expect(editor.disabled).toBe(false);
    expect(editor.value).toBe("SELECT 2;");
  });

  it("renders shell structure and query page containers", async () => {
    const root = await mountQueryRoute();

    expect(root.querySelector('[data-testid="cassie-admin-shell"]')).toBeTruthy();
    const queryPage = root.querySelector("[data-query-page]");
    expect(queryPage).toBeTruthy();
    expect(root.querySelectorAll("#main-content")).toHaveLength(1);
    expect(queryPage?.id).toBe("query-workspace-query-1");
    expect(queryPage?.getAttribute("role")).toBe("region");
    expect(queryPage?.getAttribute("aria-labelledby")).toBe("query-workspace-title-query-1");
    const schemaBrowser = root.querySelector('[aria-label="Schema browser"]');
    const schemaTree = root.querySelector('[data-testid="query-schema-tree"]');
    expect(schemaTree).toBeTruthy();
    expect(schemaBrowser?.contains(schemaTree)).toBe(true);
    expect(root.querySelector(".cassie-query-workspace [data-testid='query-schema-tree']")).toBe(
      null,
    );
    expect(root.querySelector('[data-testid="query-resizable-split-horizontal"]')).toBe(null);
    const resultsHeading = root.querySelector("#query-results-title-query-1");
    const resultsTabs = root.querySelector('[aria-label="Result tab group"]');
    expect(resultsHeading?.textContent).toBe("Results");
    expect(resultsTabs?.closest('[data-slot="toolbar"]')).not.toBeNull();
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
    await expandSchemaSection(root);

    const schemaItem = root.querySelector('[data-item-id="table:postgres.public.documents"]');
    if (!schemaItem) {
      throw new Error("Missing schema item");
    }

    const editor = editorTextarea(root);

    expect(schemaItem.getAttribute("data-item-kind")).toBe("table");
    expect(editor.value).toBe("SELECT 1 AS ready;");
  });

  it("should_replace_validation_and_result_feedback_when_the_next_action_runs", async () => {
    mockValidateSuccess();
    mockExecuteSuccess();
    const root = await mountQueryRoute();

    buttonByText(root, "Validate").click();
    await waitForText(root, "Validation passed");
    expect(root.textContent).toContain("Validation passed");

    updateEditor(root, "SELECT name FROM documents;");
    await flushUi();
    expect(root.textContent).toContain("Validation passed");

    buttonByText(root, "Run").click();
    await waitForText(root, "1 row");
    expect(root.textContent).not.toContain("Command");
    expect(root.querySelector(".cassie-query-execution-summary-command")?.textContent).toBe(
      "SELECT",
    );
    expect(root.textContent).toContain("1 row");

    updateEditor(root, "SELECT id FROM documents;");
    await flushUi();
    expect(root.textContent).toContain("1 row");
  });

  it("shows a danger toast when validation itself fails, instead of failing silently", async () => {
    mockValidateError();
    const root = await mountQueryRoute();

    buttonByText(root, "Validate").click();
    await waitForText(root, "Validation failed");

    const toast = root.querySelector('[data-slot="toast"]');
    if (!(toast instanceof HTMLElement)) {
      throw new Error("Missing validation toast");
    }
    expect(toast.getAttribute("data-variant")).toBe("danger");
    expect(toast.textContent).toContain('syntax error at or near "SELET"');
  });

  it("shows validation results as a dismissible toast, not a persistent banner", async () => {
    mockValidateSuccess();
    const root = await mountQueryRoute();

    buttonByText(root, "Validate").click();
    await waitForText(root, "Validation passed");

    const toast = root.querySelector('[data-slot="toast"]');
    if (!(toast instanceof HTMLElement)) {
      throw new Error("Missing validation toast");
    }
    expect(toast.getAttribute("data-variant")).toBe("success");

    const dismissButton = toast.querySelector('button[aria-label="Dismiss notification"]');
    if (!(dismissButton instanceof HTMLElement)) {
      throw new Error("Missing toast dismiss button");
    }
    dismissButton.click();
    await flushUi();
    expect(root.querySelector('[data-slot="toast"]')).toBeNull();
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
    if (!(container instanceof HTMLElement) || !(handle instanceof HTMLElement)) {
      throw new Error("Missing vertical split container or handle");
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
    expect(container.style.getPropertyValue("--cassie-split-size")).toBe("75%");

    handle.dispatchEvent(
      new PointerEvent("pointerup", { bubbles: true, clientX: 100, clientY: 300, pointerId: 1 }),
    );
    await flushUi();

    expect(handle.getAttribute("aria-valuenow")).toBe("75");
    expect(container.style.getPropertyValue("--cassie-split-size")).toBe("75%");
  });

  it("should_hide_empty_schema_sections_and_start_populated_sections_collapsed", async () => {
    const root = await mountQueryRoute();
    await waitForText(root, "public");

    const tablesSection = root.querySelector(
      '[data-testid="query-schema-tree-section"][data-section="tables"]',
    );
    const viewsSection = root.querySelector(
      '[data-testid="query-schema-tree-section"][data-section="views"]',
    );
    if (!tablesSection) {
      throw new Error("Missing tables schema section");
    }

    const tablesToggle = tablesSection.querySelector("[aria-expanded]");
    if (!(tablesToggle instanceof HTMLElement)) {
      throw new Error("Missing tables section toggle");
    }

    expect(viewsSection).toBeNull();
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

    tablesToggle.click();
    await flushUi();
    expect(tablesToggle.getAttribute("aria-expanded")).toBe("false");
  });

  it("selects a schema item without overwriting the SQL editor", async () => {
    const root = await mountQueryRoute();
    await expandSchemaSection(root);
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

  it("should_expand_the_active_database_namespace_while_groups_remain_collapsed", async () => {
    const root = await mountQueryRoute();
    await waitForText(root, "public");

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
    const tablesToggle = namespace.querySelector<HTMLElement>(
      '[data-testid="query-schema-tree-section"][data-section="tables"] [aria-expanded]',
    );
    expect(tablesToggle?.getAttribute("aria-expanded")).toBe("false");
    expect(namespace.querySelector('[data-item-id="table:postgres.public.documents"]')).toBe(null);

    namespaceToggle.click();
    await flushUi();
    expect(namespaceToggle.getAttribute("aria-expanded")).toBe("false");

    namespaceToggle.click();
    await flushUi();
    expect(namespaceToggle.getAttribute("aria-expanded")).toBe("true");
  });

  it("renders a kind icon on schema items", async () => {
    const root = await mountQueryRoute();
    await expandSchemaSection(root);

    const item = root.querySelector('[data-item-id="table:postgres.public.documents"]');
    expect(item?.querySelector("svg")).toBeTruthy();
  });

  it("expands a table to show its columns, with a key icon on the primary key", async () => {
    mockQuerySchemaWithColumnsSuccess();
    const root = await mountQueryRoute();
    await expandSchemaSection(root);

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
