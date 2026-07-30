import { afterEach, beforeEach, vi } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";

import RootLayout from "@/pages/_layout";
import AppLayout from "@/pages/app/_layout";
import QueryPage from "@/pages/app/query";
import { type ColumnMeta, type QuerySchemaResponse } from "@/adapters";
import { fetchMock, mockJsonResponse, resetFetchMock } from "./mock-fetch";
import { saveQueryWorkspace } from "@/features/query/query-tabs";
import { queryService } from "@/features/query/query-service";
import { querySchemaResponse } from "../fixtures/query-schema";
import { explainPlan } from "../fixtures/query-explain-plan";
import { createTestRouteRegistry } from "./test-route-registry";

function mockQuerySchemaSuccess() {
  mockJsonResponse("/api/v1/admin/catalog", querySchemaResponse);
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
    registry: createTestRouteRegistry([
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
    ]),
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

export {
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
  mockSchemaChangingCommandSuccess,
  mockValidateError,
  mockValidateSuccess,
  mountQueryRoute,
  updateEditor,
  waitForText,
};
