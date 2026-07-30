import { describe, expect, it } from "vite-plus/test";

import { queryService } from "@/features/query/query-service";
import { saveQueryWorkspace } from "@/features/query/query-tabs";
import {
  fetchMock,
  flushUi,
  mockJsonResponse,
  mountQueryRoute,
  waitForText,
} from "./support/query-page-harness";

function catalogRequests() {
  return fetchMock.mock.calls
    .map(([request]) => request)
    .filter((request) => new URL(request.url).pathname === "/api/v1/admin/catalog");
}

async function waitForCatalogState(root: Element, database: string, state: string) {
  for (let attempt = 0; attempt < 10; attempt += 1) {
    await flushUi();
    const branch = root.querySelector(
      `[data-testid="query-schema-tree-database"][data-database="${database}"]`,
    );
    if (branch?.getAttribute("data-load-state") === state) return branch;
  }

  throw new Error(`Timed out waiting for ${database} catalog state ${state}`);
}

describe("query page schema loading", () => {
  it("should_keep_discovered_databases_collapsed_and_idle_without_an_active_query", async () => {
    // Arrange
    saveQueryWorkspace("anonymous", { version: 1, activeTabId: null, tabs: [] });
    mockJsonResponse("/api/v1/admin/databases", [{ name: "Database1" }, { name: "Database2" }]);
    queryService.invalidateSchema("Database1");
    queryService.invalidateSchema("Database2");

    // Act
    const root = await mountQueryRoute();
    await waitForText(root, "Database2");

    // Assert
    const branches = root.querySelectorAll('[data-testid="query-schema-tree-database"]');
    expect(branches).toHaveLength(2);
    for (const branch of branches) {
      expect(branch.getAttribute("data-load-state")).toBe("idle");
      expect(branch.querySelector("[aria-expanded]")?.getAttribute("aria-expanded")).toBe("false");
    }
    expect(catalogRequests()).toHaveLength(0);
  });

  it("should_load_only_the_active_database_and_leave_siblings_collapsed", async () => {
    // Arrange
    mockJsonResponse("/api/v1/admin/databases", [{ name: "Database1" }, { name: "Database2" }]);
    queryService.invalidateSchema("Database1");
    queryService.invalidateSchema("Database2");
    saveQueryWorkspace("anonymous", {
      version: 1,
      activeTabId: "query-2",
      tabs: [
        {
          id: "query-1",
          ordinal: 1,
          title: "Query 1",
          database: "Database1",
          sql: "SELECT 1;",
        },
        {
          id: "query-2",
          ordinal: 2,
          title: "Query 2",
          database: "Database2",
          sql: "SELECT 2;",
        },
      ],
    });

    // Act
    const root = await mountQueryRoute();
    const activeBranch = await waitForCatalogState(root, "Database2", "loaded");
    const inactiveBranch = root.querySelector(
      '[data-testid="query-schema-tree-database"][data-database="Database1"]',
    );

    // Assert
    expect(activeBranch.querySelector("[aria-expanded]")?.getAttribute("aria-expanded")).toBe(
      "true",
    );
    expect(inactiveBranch?.getAttribute("data-load-state")).toBe("idle");
    expect(inactiveBranch?.querySelector("[aria-expanded]")?.getAttribute("aria-expanded")).toBe(
      "false",
    );
    expect(catalogRequests()).toHaveLength(1);
    expect(new URL(catalogRequests()[0]?.url ?? "").searchParams.get("database")).toBe("Database2");
  });

  it("should_load_remaining_catalogs_when_schema_search_becomes_nonempty", async () => {
    // Arrange
    mockJsonResponse("/api/v1/admin/databases", [{ name: "Database1" }, { name: "Database2" }]);
    queryService.invalidateSchema("Database1");
    queryService.invalidateSchema("Database2");
    saveQueryWorkspace("anonymous", {
      version: 1,
      activeTabId: "query-1",
      tabs: [
        {
          id: "query-1",
          ordinal: 1,
          title: "Query 1",
          database: "Database1",
          sql: "SELECT 1;",
        },
      ],
    });
    const root = await mountQueryRoute();
    await waitForCatalogState(root, "Database1", "loaded");
    const search = root.querySelector<HTMLInputElement>('[aria-label="Filter schema objects"]');
    if (!search) throw new Error("Missing schema search");

    // Act
    search.value = "documents";
    search.dispatchEvent(new InputEvent("input", { bubbles: true }));
    const loadedSibling = await waitForCatalogState(root, "Database2", "loaded");

    // Assert
    expect(loadedSibling.querySelector("[aria-expanded]")?.getAttribute("aria-expanded")).toBe(
      "false",
    );
    expect(catalogRequests()).toHaveLength(2);
    expect(
      catalogRequests().map((request) => new URL(request.url).searchParams.get("database")),
    ).toEqual(["Database1", "Database2"]);
  });
});
