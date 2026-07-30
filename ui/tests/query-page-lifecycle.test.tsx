import { describe, expect, it } from "vite-plus/test";

import { saveQueryWorkspace } from "@/features/query/query-tabs";
import {
  buttonByText,
  editorTextarea,
  fetchMock,
  flushUi,
  mockJsonResponse,
  mockSchemaChangingCommandSuccess,
  mockValidateSuccess,
  mountQueryRoute,
  updateEditor,
  waitForText,
} from "./support/query-page-harness";

describe("admin query page lifecycle and actions", () => {
  it("should_keep_the_editor_mounted_when_query_actions_update_state", async () => {
    // Arrange
    mockValidateSuccess();
    const root = await mountQueryRoute();
    const editor = editorTextarea(root);
    const panel = root.querySelector('[data-testid="query-editor-panel"]');

    // Act
    buttonByText(root, "Validate").click();
    await waitForText(root, "Validation passed");

    // Assert
    expect(root.querySelector('[data-testid="query-editor-panel"]')).toBe(panel);
    expect(root.querySelector('[data-query-editor="fallback"] textarea')).toBe(editor);
  });

  it("should_create_a_database_from_the_database_tree", async () => {
    // Arrange
    saveQueryWorkspace("anonymous", { version: 1, activeTabId: null, tabs: [] });
    mockJsonResponse(
      "/api/v1/admin/databases",
      { name: "analytics" },
      { method: "POST", status: 201 },
    );
    const root = await mountQueryRoute();

    // Act
    (root.querySelector('button[aria-label="Create database"]') as HTMLButtonElement).click();
    const input = root.querySelector("#create-database-name") as HTMLInputElement;
    input.value = "analytics";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    buttonByText(root.querySelector('[role="dialog"]') ?? root, "Create database").click();
    await waitForText(root, "Query 1");

    // Assert
    const request = fetchMock.mock.calls
      .map(([candidate]) => candidate)
      .find(
        (candidate) =>
          candidate.method === "POST" &&
          new URL(candidate.url).pathname === "/api/v1/admin/databases",
      );
    expect(await request?.clone().json()).toEqual({ name: "analytics" });
  });

  it("should_keep_the_create_database_dialog_mounted_while_typing", async () => {
    // Arrange
    const root = await mountQueryRoute();
    (root.querySelector('button[aria-label="Create database"]') as HTMLButtonElement).click();
    const dialog = root.querySelector('[role="dialog"]');
    const overlay = root.querySelector('[data-slot="dialog-overlay"]');
    const input = root.querySelector("#create-database-name") as HTMLInputElement;

    // Act
    input.value = "a";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await flushUi();

    // Assert
    expect(root.querySelector('[role="dialog"]')).toBe(dialog);
    expect(root.querySelector('[data-slot="dialog-overlay"]')).toBe(overlay);
    expect(root.querySelector("#create-database-name")).toBe(input);
  });

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
    expect(heading?.textContent).toBe("Query 1 query workspace");
    expect(heading?.classList.contains("sr-only")).toBe(true);
    expect(root.querySelector('[data-slot="page-header"]')).toBe(null);
    expect(root.querySelector('[data-testid="query-starters"]')).toBe(null);
    expect(root.querySelector("[data-query-page]")?.getAttribute("aria-labelledby")).toBe(
      "query-workspace-title-query-1",
    );
    expect(root.querySelector('[aria-label="Query tabs"]')).toBeNull();
    expect(root.textContent).toContain("My Queries");
    const databaseTree = root.querySelector('[data-testid="query-schema-tree"]');
    const myQueries = root.querySelector('[aria-labelledby="my-queries-title"]');
    expect(databaseTree).not.toBeNull();
    expect(myQueries).not.toBeNull();
    if (databaseTree && myQueries)
      expect(
        databaseTree.compareDocumentPosition(myQueries) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
  });
});
