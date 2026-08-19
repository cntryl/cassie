import { afterEach, describe, expect, it } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";
import { state } from "@askrjs/askr";
import { createDataRuntime } from "@askrjs/askr/data";

import { QuerySchemaTree } from "@/components/query/query-schema-tree";
import type { DatabaseCatalogEntry } from "@/features/query/database-catalog-controller";
import type { QuerySchemaDatabase } from "@/features/query/query-models";
import { createTestRouteRegistry } from "./support/test-route-registry";

const schema: QuerySchemaDatabase[] = [
  {
    id: "postgres",
    label: "postgres",
    namespaces: [
      {
        id: "public",
        label: "public",
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
          { id: "views", label: "Views", items: [] },
        ],
      },
    ],
  },
];

function loadedCatalogs(databases: QuerySchemaDatabase[]): DatabaseCatalogEntry[] {
  return databases.map((database) => ({
    canonicalName: database.id.toLocaleLowerCase(),
    name: database.label,
    status: "loaded",
    expanded: true,
    database,
  }));
}

const schemaTreeActions = {
  onSetDatabaseExpanded: () => {},
  onSearchCatalogs: () => {},
  onRetryDatabase: () => {},
};

async function flushUi() {
  await new Promise<void>((resolve) => queueMicrotask(() => resolve()));
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

async function mountSchemaTree() {
  cleanupApp("app");
  document.body.innerHTML = '<div id="app"></div>';
  const root = document.getElementById("app");
  if (!root) {
    throw new Error("Missing test app root");
  }

  await createSPA({
    root,
    dataRuntime: createDataRuntime(),
    registry: createTestRouteRegistry([
      {
        path: "/",
        handler: () => (
          <QuerySchemaTree
            catalogs={loadedCatalogs(schema)}
            activeDatabase="postgres"
            onSelectItem={() => {}}
            {...schemaTreeActions}
          />
        ),
      },
    ]),
  });

  await flushUi();
  return root;
}

let updateDynamicSchema: ((next: QuerySchemaDatabase[]) => void) | null = null;

function DynamicSchemaTree() {
  const [currentSchema, setCurrentSchema] = state(schema);
  updateDynamicSchema = setCurrentSchema;
  return (
    <QuerySchemaTree
      catalogs={() => loadedCatalogs(currentSchema())}
      activeDatabase="postgres"
      onSelectItem={() => {}}
      {...schemaTreeActions}
    />
  );
}

async function mountDynamicSchemaTree() {
  cleanupApp("app");
  document.body.innerHTML = '<div id="app"></div>';
  const root = document.getElementById("app");
  if (!root) {
    throw new Error("Missing test app root");
  }

  await createSPA({
    root,
    dataRuntime: createDataRuntime(),
    registry: createTestRouteRegistry([{ path: "/", handler: DynamicSchemaTree }]),
  });
  await flushUi();
  return root;
}

function searchInputOf(root: Element) {
  const input = root.querySelector(
    '[aria-label="Filter schema objects"]',
  ) as HTMLInputElement | null;
  if (!input) {
    throw new Error("Missing schema search input");
  }
  return input;
}

function typeInto(input: HTMLInputElement, value: string) {
  input.value = value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

afterEach(() => {
  cleanupApp("app");
  document.body.innerHTML = "";
  updateDynamicSchema = null;
});

describe("schema tree search", () => {
  it("filters items across sections and excludes non-matching items", async () => {
    const root = await mountSchemaTree();

    typeInto(searchInputOf(root), "doc");
    await flushUi();

    const body = root.querySelector('[aria-label="Schema sections"]');
    expect(body?.getAttribute("data-schema-mode")).toBe("results");
    const results = root.querySelector('[data-testid="query-schema-tree-results"]');
    expect(results?.querySelector('[data-item-id="table:postgres.public.documents"]')).toBeTruthy();
    expect(results?.querySelector('[data-item-id="table:postgres.public.accounts"]')).toBe(null);
  });

  it("shows a no-matches message when nothing matches", async () => {
    const root = await mountSchemaTree();

    typeInto(searchInputOf(root), "no-such-object");
    await flushUi();

    const body = root.querySelector('[aria-label="Schema sections"]');
    expect(body?.getAttribute("data-schema-mode")).toBe("empty");
    expect(root.querySelector(".cassie-query-schema-empty-search")?.textContent).toContain(
      "No matches",
    );
  });

  it("clears the filter and restores the full tree", async () => {
    const root = await mountSchemaTree();
    const input = searchInputOf(root);

    typeInto(input, "doc");
    await flushUi();

    const clearButton = root.querySelector(
      '[aria-label="Clear filter"]',
    ) as HTMLButtonElement | null;
    if (!clearButton) {
      throw new Error("Missing clear filter button");
    }
    clearButton.click();
    await flushUi();

    const body = root.querySelector('[aria-label="Schema sections"]');
    expect(input.value).toBe("");
    expect(body?.getAttribute("data-schema-mode")).toBe("tree");
    const tables = root.querySelector(
      '[data-testid="query-schema-tree-section"][data-section="tables"]',
    );
    const toggle = tables?.querySelector<HTMLElement>("[aria-expanded]");
    expect(toggle?.getAttribute("aria-expanded")).toBe("false");
    toggle?.click();
    await flushUi();
    expect(tables?.querySelector('[data-item-id="table:postgres.public.accounts"]')).toBeTruthy();
  });

  it("should_refresh_active_search_results_given_new_schema_data", async () => {
    // Arrange
    const root = await mountDynamicSchemaTree();
    typeInto(searchInputOf(root), "archive");
    await flushUi();
    expect(
      root.querySelector('[aria-label="Schema sections"]')?.getAttribute("data-schema-mode"),
    ).toBe("empty");

    // Act
    const nextSchema = structuredClone(schema);
    nextSchema[0]?.namespaces[0]?.sections[0]?.items.push({
      id: "table:postgres.public.archive",
      kind: "table",
      label: "postgres.public.archive",
      database: "postgres",
      schema: "public",
      name: "archive",
      columns: [],
      metadata: "1 column",
    });
    updateDynamicSchema?.(nextSchema);
    await flushUi();

    // Assert
    expect(
      root.querySelector('[aria-label="Schema sections"]')?.getAttribute("data-schema-mode"),
    ).toBe("results");
    expect(root.querySelector('[data-item-id="table:postgres.public.archive"]')).toBeTruthy();
  });
});
