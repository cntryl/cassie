import { describe, expect, it, vi } from "vite-plus/test";
import { navigate } from "@askrjs/askr/router";

import { DatabaseCatalogController } from "@/features/query/database-catalog-controller";
import { saveQueryWorkspace } from "@/features/query/query-tabs";
import {
  buttonByText,
  editorTextarea,
  fetchMock,
  flushUi,
  mockExecuteSuccess,
  mockExplainSuccess,
  mockJsonResponse,
  mockResponseHandler,
  mockSchemaChangingCommandSuccess,
  mockValidateSuccess,
  mountQueryRoute,
  updateEditor,
  waitForText,
} from "./support/query-page-harness";

describe("admin query page lifecycle and actions", () => {
  it("should_cancel_the_active_operation_id_and_abort_its_request_when_stopped", async () => {
    // Arrange
    let executeRequest: Request | undefined;
    mockResponseHandler(
      "/api/v1/admin/query-executions",
      (request) => {
        executeRequest = request;
        return new Promise<Response>((_resolve, reject) => {
          request.signal.addEventListener("abort", () => reject(request.signal.reason), {
            once: true,
          });
        });
      },
      { method: "POST" },
    );
    mockResponseHandler(
      "/api/v1/admin/query-operations/active-operation",
      () =>
        new Response(JSON.stringify({ operation_id: "active-operation", cancelled: true }), {
          headers: { "content-type": "application/json" },
        }),
      { method: "DELETE" },
    );
    const randomUuid = vi
      .spyOn(crypto, "randomUUID")
      .mockReturnValue("active-operation" as `${string}-${string}-${string}-${string}-${string}`);
    const root = await mountQueryRoute();
    expect(buttonByText(root, "Run").disabled).toBe(false);

    // Act
    buttonByText(root, "Run").click();
    await flushUi();
    expect(root.querySelector("[data-query-page]")?.getAttribute("data-operation-active")).toBe(
      "true",
    );
    await waitForText(root, "Running query");
    buttonByText(root, "Stop").click();
    await waitForText(root, "Run");
    await flushUi();

    // Assert
    const cancellation = fetchMock.mock.calls
      .map(([request]) => request)
      .find((request) => request.method === "DELETE");
    expect(new URL(cancellation?.url ?? window.location.href).pathname).toBe(
      "/api/v1/admin/query-operations/active-operation",
    );
    expect(executeRequest?.signal.aborted).toBe(true);
    randomUuid.mockRestore();
  });

  it("should_cancel_a_running_query_before_removing_its_workspace", async () => {
    // Arrange
    mockResponseHandler(
      "/api/v1/admin/query-executions",
      (request) =>
        new Promise<Response>((_resolve, reject) => {
          request.signal.addEventListener("abort", () => reject(request.signal.reason), {
            once: true,
          });
        }),
      { method: "POST" },
    );
    let workspacePresentWhenCancelled = false;
    mockResponseHandler(
      "/api/v1/admin/query-operations/close-operation",
      () => {
        workspacePresentWhenCancelled = root.querySelector("#saved-query-query-1") !== null;
        return new Response(JSON.stringify({ operation_id: "close-operation", cancelled: true }), {
          headers: { "content-type": "application/json" },
        });
      },
      { method: "DELETE" },
    );
    vi.spyOn(crypto, "randomUUID").mockReturnValue(
      "close-operation" as `${string}-${string}-${string}-${string}-${string}`,
    );
    const root = await mountQueryRoute();
    buttonByText(root, "Run").click();
    await waitForText(root, "Running query");

    // Act
    const deleteButtons = document.querySelectorAll<HTMLButtonElement>(
      'button[aria-label="Delete Query 1"]',
    );
    expect(deleteButtons).toHaveLength(1);
    deleteButtons[deleteButtons.length - 1]?.click();
    await waitForText(document.body, "running operation will be cancelled first");
    buttonByText(document.body, "Delete query").click();
    await waitForText(root, "Choose a database");

    // Assert
    expect(workspacePresentWhenCancelled).toBe(true);
    expect(root.querySelector("#saved-query-query-1")).toBeNull();
  });

  it.each([404, 409])(
    "should_acknowledge_a_%s_cancellation_response_as_completed",
    async (status) => {
      // Arrange
      mockResponseHandler(
        "/api/v1/admin/query-executions",
        (request) =>
          new Promise<Response>((_resolve, reject) => {
            request.signal.addEventListener("abort", () => reject(request.signal.reason), {
              once: true,
            });
          }),
        { method: "POST" },
      );
      mockJsonResponse(
        "/api/v1/admin/query-operations/acknowledged-operation",
        { error: "operation already finished" },
        { method: "DELETE", status },
      );
      vi.spyOn(crypto, "randomUUID").mockReturnValue(
        "acknowledged-operation" as `${string}-${string}-${string}-${string}-${string}`,
      );
      const root = await mountQueryRoute();
      buttonByText(root, "Run").click();
      await waitForText(root, "Running query");

      // Act
      buttonByText(root, "Stop").click();
      await waitForText(root, "Run");

      // Assert
      expect(root.textContent).not.toContain("Try stopping again");
      expect(buttonByText(root, "Run").disabled).toBe(false);
    },
  );

  it("should_keep_a_failed_cancellation_retryable", async () => {
    // Arrange
    mockResponseHandler(
      "/api/v1/admin/query-executions",
      (request) =>
        new Promise<Response>((_resolve, reject) => {
          request.signal.addEventListener("abort", () => reject(request.signal.reason), {
            once: true,
          });
        }),
      { method: "POST" },
    );
    mockJsonResponse(
      "/api/v1/admin/query-operations/retry-operation",
      { error: "cancellation unavailable" },
      { method: "DELETE", status: 503 },
    );
    vi.spyOn(crypto, "randomUUID").mockReturnValue(
      "retry-operation" as `${string}-${string}-${string}-${string}-${string}`,
    );
    const root = await mountQueryRoute();
    buttonByText(root, "Run").click();
    await waitForText(root, "Running query");

    // Act
    buttonByText(root, "Stop").click();
    await waitForText(root, "Try stopping again");

    // Assert
    expect(buttonByText(root, "Stop").disabled).toBe(false);
    expect(root.textContent).toContain("Try stopping again");
  });

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

  it("should_disable_query_actions_immediately_when_the_editor_is_cleared", async () => {
    // Arrange
    const root = await mountQueryRoute();

    // Act
    updateEditor(root, "");
    await flushUi();

    // Assert
    expect(buttonByText(root, "Run").disabled).toBe(true);
    expect(buttonByText(root, "Validate").disabled).toBe(true);
    expect(buttonByText(root, "Explain").disabled).toBe(true);
  });

  it("should_enable_query_actions_immediately_when_text_is_entered", async () => {
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
          sql: "",
        },
      ],
    });
    const root = await mountQueryRoute();

    // Act
    updateEditor(root, "SELECT 2;");
    await flushUi();

    // Assert
    expect(buttonByText(root, "Run").disabled).toBe(false);
    expect(buttonByText(root, "Validate").disabled).toBe(false);
    expect(buttonByText(root, "Explain").disabled).toBe(false);
  });

  it("should_preserve_query_feedback_across_a_workspace_tab_round_trip", async () => {
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
          sql: "SELECT 1 AS ready;",
        },
        {
          id: "query-2",
          ordinal: 2,
          title: "Query 2",
          database: "postgres",
          sql: "SELECT 2 AS ready;",
        },
      ],
    });
    mockValidateSuccess();
    mockExecuteSuccess();
    mockExplainSuccess();
    const root = await mountQueryRoute();
    buttonByText(root, "Run").click();
    await waitForText(root, "1 row");
    buttonByText(root, "Explain").click();
    await waitForText(root, "Read with idx_documents_title");
    buttonByText(root, "Grid").click();
    await flushUi();
    buttonByText(root, "Validate").click();
    await waitForText(root, "Validation passed");

    // Act
    document.getElementById("saved-query-query-2")?.click();
    await flushUi();
    document.getElementById("saved-query-query-1")?.click();
    await flushUi();

    // Assert
    expect(root.querySelector('[data-tab-content="results"] table')).not.toBeNull();
    expect(root.textContent).toContain("Validation passed");
    buttonByText(root, "Plan").click();
    await flushUi();
    expect(root.textContent).toContain("Read with idx_documents_title");
  });

  it("should_dispose_database_catalog_requests_when_the_page_unmounts", async () => {
    // Arrange
    const dispose = vi.spyOn(DatabaseCatalogController.prototype, "dispose");
    const root = await mountQueryRoute();
    expect(root.querySelector("[data-query-page]")).not.toBeNull();

    // Act
    navigate("/other");
    await waitForText(root, "Other route");

    // Assert
    expect(dispose).toHaveBeenCalled();
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
    for (let attempt = 0; attempt < 10; attempt += 1) {
      const databaseReads = fetchMock.mock.calls.filter(
        ([candidate]) =>
          candidate.method === "GET" &&
          new URL(candidate.url).pathname === "/api/v1/admin/databases",
      );
      if (databaseReads.length >= 2) break;
      await flushUi();
    }

    // Assert
    const request = fetchMock.mock.calls
      .map(([candidate]) => candidate)
      .find(
        (candidate) =>
          candidate.method === "POST" &&
          new URL(candidate.url).pathname === "/api/v1/admin/databases",
      );
    expect(await request?.clone().json()).toEqual({ name: "analytics" });
    expect(
      fetchMock.mock.calls.filter(
        ([candidate]) =>
          candidate.method === "GET" &&
          new URL(candidate.url).pathname === "/api/v1/admin/databases",
      ),
    ).toHaveLength(2);
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
