import { cleanupApp, createSPA } from "@askrjs/askr/boot";
import { createDataRuntime } from "@askrjs/askr/data";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { QuerySchemaTree } from "@/components/query/query-schema-tree";
import type { DatabaseCatalogEntry } from "@/features/query/database-catalog-controller";
import { createTestRouteRegistry } from "./support/test-route-registry";

async function flushUi() {
  await new Promise<void>((resolve) => queueMicrotask(resolve));
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

async function mountCatalog(catalog: DatabaseCatalogEntry, onRetry = () => {}) {
  cleanupApp("app");
  document.body.innerHTML = '<div id="app"></div>';
  const root = document.getElementById("app");
  if (!root) throw new Error("Missing app root");

  await createSPA({
    root,
    dataRuntime: createDataRuntime(),
    registry: createTestRouteRegistry([
      {
        path: "/",
        handler: () => (
          <QuerySchemaTree
            catalogs={[catalog]}
            activeDatabase={catalog.name}
            onSelectItem={() => {}}
            onSetDatabaseExpanded={() => {}}
            onSearchCatalogs={() => {}}
            onRetryDatabase={onRetry}
          />
        ),
      },
    ]),
  });
  await flushUi();
  return root;
}

afterEach(() => {
  cleanupApp("app");
  document.body.innerHTML = "";
});

describe("schema tree catalog states", () => {
  it("should_render_loading_content_inside_the_expanded_database_branch", async () => {
    // Arrange
    const catalog: DatabaseCatalogEntry = {
      canonicalName: "postgres",
      name: "postgres",
      status: "loading",
      expanded: true,
    };

    // Act
    const root = await mountCatalog(catalog);

    // Assert
    const branch = root.querySelector('[data-testid="query-schema-tree-database"]');
    expect(branch?.getAttribute("data-load-state")).toBe("loading");
    expect(branch?.textContent).toContain("Loading catalog…");
  });

  it("should_render_an_empty_message_for_a_loaded_database_without_schema_objects", async () => {
    // Arrange
    const catalog: DatabaseCatalogEntry = {
      canonicalName: "postgres",
      name: "postgres",
      status: "loaded",
      expanded: true,
      database: { id: "postgres", label: "postgres", namespaces: [] },
    };

    // Act
    const root = await mountCatalog(catalog);

    // Assert
    expect(root.textContent).toContain("No schema objects.");
    expect(root.querySelector('[data-testid="query-schema-tree-namespace"]')).toBeNull();
  });

  it("should_offer_retry_inside_a_failed_database_branch", async () => {
    // Arrange
    const retry = vi.fn();
    const catalog: DatabaseCatalogEntry = {
      canonicalName: "postgres",
      name: "postgres",
      status: "error",
      expanded: true,
      error: new Error("catalog unavailable"),
    };
    const root = await mountCatalog(catalog, retry);

    // Act
    root.querySelector<HTMLButtonElement>(".cassie-query-schema-database-status button")?.click();
    await flushUi();

    // Assert
    expect(root.textContent).toContain("catalog unavailable");
    expect(retry).toHaveBeenCalledWith("postgres");
  });
});
