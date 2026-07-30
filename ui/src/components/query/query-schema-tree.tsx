import { For } from "@askrjs/askr/control";
import { state } from "@askrjs/askr";
import { Button, Text } from "@askrjs/themes/components";
import { PlusIcon, SearchIcon, XIcon } from "@askrjs/lucide";

import type { DatabaseCatalogEntry } from "@/features/query/database-catalog-controller";
import type { QuerySchemaDatabase, QuerySchemaItem } from "@/features/query/query-models";
import { QuerySchemaTreeDatabase } from "./query-schema-tree-database";
import { QuerySchemaTreeItem } from "./query-schema-tree-item";

interface QuerySchemaTreeProps {
  catalogs: DatabaseCatalogEntry[] | (() => DatabaseCatalogEntry[]);
  activeDatabase?: string | (() => string | undefined);
  selectedItemId?: string | (() => string | undefined);
  onSelectItem: (item: QuerySchemaItem) => void;
  onCreateDatabase?: () => void;
  onSetDatabaseExpanded: (database: string, expanded: boolean) => void;
  onSearchCatalogs: () => void;
  onRetryDatabase: (database: string) => void;
}

interface QuerySchemaSearchGroup {
  id: string;
  label: string;
  items: QuerySchemaItem[];
}

function filterDatabases(
  databases: QuerySchemaDatabase[],
  query: string,
): QuerySchemaSearchGroup[] {
  const normalized = query.trim().toLowerCase();
  if (!normalized) {
    return [];
  }

  const groups: QuerySchemaSearchGroup[] = [];
  for (const database of databases) {
    for (const namespace of database.namespaces) {
      for (const section of namespace.sections) {
        const items = section.items.filter((item) => item.label.toLowerCase().includes(normalized));
        if (items.length === 0) {
          continue;
        }

        groups.push({
          id: `${database.id}:${namespace.id}:${section.id}`,
          label: `${database.label} / ${namespace.label} / ${section.label}`,
          items,
        });
      }
    }
  }

  return groups;
}

export function QuerySchemaTree({
  catalogs,
  activeDatabase,
  selectedItemId,
  onSelectItem,
  onCreateDatabase,
  onSetDatabaseExpanded,
  onSearchCatalogs,
  onRetryDatabase,
}: QuerySchemaTreeProps) {
  const getCatalogs = () => (typeof catalogs === "function" ? catalogs() : catalogs);
  const getActiveDatabase = () =>
    typeof activeDatabase === "function" ? activeDatabase() : activeDatabase;
  const getSelectedItemId = () =>
    typeof selectedItemId === "function" ? selectedItemId() : selectedItemId;

  const [query, setQuery] = state("");

  function handleQueryInput(event: InputEvent) {
    const target = event.target;
    if (target instanceof HTMLInputElement) {
      const value = target.value;
      setQuery(value);
      if (value.trim()) onSearchCatalogs();
    }
  }

  function clearQuery() {
    setQuery("");
  }

  const loadedDatabases = () =>
    getCatalogs()
      .filter(
        (catalog): catalog is DatabaseCatalogEntry & { database: QuerySchemaDatabase } =>
          catalog.status === "loaded" && catalog.database !== undefined,
      )
      .map((catalog) => catalog.database);
  const matches = () => filterDatabases(loadedDatabases(), query());
  const isSearching = () => query().trim().length > 0;
  const hasMatches = () => matches().length > 0;
  const pendingCatalogCount = () =>
    getCatalogs().filter((catalog) => catalog.status === "idle" || catalog.status === "loading")
      .length;
  const settledCatalogCount = () => getCatalogs().length - pendingCatalogCount();
  const failedCatalogs = () => getCatalogs().filter((catalog) => catalog.status === "error");
  const schemaMode = () => {
    if (!isSearching()) return "tree";
    if (hasMatches() || failedCatalogs().length > 0) return "results";
    return pendingCatalogCount() > 0 ? "loading" : "empty";
  };

  return (
    <section
      class="cassie-query-schema-tree"
      data-testid="query-schema-tree"
      aria-label="Schema objects"
    >
      <header class="cassie-query-schema-tree-header">
        <div class="cassie-query-schema-tree-title-row">
          <Text size="sm" weight="semibold">
            Databases
          </Text>
          {onCreateDatabase ? (
            <Button
              type="button"
              size="icon"
              variant="ghost"
              aria-label="Create database"
              title="Create database"
              onPress={onCreateDatabase}
            >
              <PlusIcon size={14} aria-hidden="true" />
            </Button>
          ) : null}
        </div>
        <div class="cassie-query-schema-search">
          <SearchIcon class="cassie-query-schema-search-icon" size={13} aria-hidden="true" />
          <input
            type="search"
            class="cassie-query-schema-search-input"
            placeholder="Filter tables, views, indexes…"
            aria-label="Filter schema objects"
            value={query()}
            onInput={handleQueryInput}
          />
          <button
            type="button"
            class="cassie-query-schema-search-clear"
            aria-label="Clear filter"
            hidden={!isSearching()}
            style={{ display: isSearching() ? undefined : "none" }}
            onClick={clearQuery}
          >
            <XIcon size={12} />
          </button>
        </div>
      </header>
      <div
        class="cassie-query-schema-tree-body"
        role="region"
        aria-label="Schema sections"
        data-schema-mode={schemaMode()}
      >
        <div
          class="cassie-query-schema-tree-body-root"
          data-testid="query-schema-tree-normal"
          data-schema-panel="normal"
        >
          <For each={getCatalogs} by={(catalog) => catalog.canonicalName}>
            {(catalog) => (
              <QuerySchemaTreeDatabase
                catalog={catalog}
                active={catalog.canonicalName === getActiveDatabase()?.trim().toLocaleLowerCase()}
                selectedItemId={getSelectedItemId}
                onSelectItem={onSelectItem}
                onSetExpanded={onSetDatabaseExpanded}
                onRetry={onRetryDatabase}
              />
            )}
          </For>
        </div>

        <p class="cassie-query-schema-empty-search">No matches for “{query()}”.</p>

        <Text
          class="cassie-query-schema-search-progress"
          size="sm"
          tone="muted"
          role="status"
          hidden={!isSearching() || pendingCatalogCount() === 0}
          style={{
            display: isSearching() && pendingCatalogCount() > 0 ? undefined : "none",
          }}
        >
          Loading catalogs {settledCatalogCount()} of {getCatalogs().length}…
        </Text>

        <div
          class="cassie-query-schema-tree-body-root"
          data-testid="query-schema-tree-results"
          data-schema-panel="results"
        >
          <For each={matches} by={(group) => group.id}>
            {(group) => (
              <div class="cassie-query-schema-search-group" data-testid="query-schema-search-group">
                <span class="cassie-query-schema-search-group-label">{group.label}</span>
                <ul class="cassie-query-schema-section-list">
                  <For each={group.items} by={(item) => item.id}>
                    {(item) => (
                      <QuerySchemaTreeItem
                        item={item}
                        selected={() => getSelectedItemId() === item.id}
                        onSelectItem={onSelectItem}
                      />
                    )}
                  </For>
                </ul>
              </div>
            )}
          </For>
          <For each={failedCatalogs} by={(catalog) => catalog.canonicalName}>
            {(catalog) => (
              <div
                class="cassie-query-schema-search-group cassie-query-schema-search-failure"
                data-database={catalog.name}
              >
                <span class="cassie-query-schema-search-group-label">{catalog.name}</span>
                <Text size="sm" tone="muted">
                  Catalog unavailable.
                </Text>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onPress={() => onRetryDatabase(catalog.name)}
                >
                  Retry
                </Button>
              </div>
            )}
          </For>
        </div>
      </div>
    </section>
  );
}
