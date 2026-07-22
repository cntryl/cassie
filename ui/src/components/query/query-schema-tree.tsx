import { For } from "@askrjs/askr/control";
import { state } from "@askrjs/askr";
import { Text } from "@askrjs/themes/components";
import { SearchIcon, XIcon } from "@askrjs/lucide";

import type { QuerySchemaDatabase, QuerySchemaItem } from "@/features/query/query-models";
import { QuerySchemaTreeDatabase } from "./query-schema-tree-database";
import { QuerySchemaTreeItem } from "./query-schema-tree-item";

interface QuerySchemaTreeProps {
  schema: QuerySchemaDatabase[] | (() => QuerySchemaDatabase[]);
  selectedItemId?: string | (() => string | undefined);
  onSelectItem: (item: QuerySchemaItem) => void;
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

export function QuerySchemaTree({ schema, selectedItemId, onSelectItem }: QuerySchemaTreeProps) {
  const getSchema = () => (typeof schema === "function" ? schema() : schema);
  const getSelectedItemId = () =>
    typeof selectedItemId === "function" ? selectedItemId() : selectedItemId;

  const [query, setQuery] = state("");

  function handleQueryInput(event: InputEvent) {
    const target = event.target;
    if (target instanceof HTMLInputElement) {
      const value = target.value;
      setQuery(value);
    }
  }

  function clearQuery() {
    setQuery("");
  }

  const matches = () => filterDatabases(getSchema(), query());
  const isSearching = () => query().trim().length > 0;
  const hasMatches = () => matches().length > 0;
  const schemaMode = () => (isSearching() ? (hasMatches() ? "results" : "empty") : "tree");

  return (
    <section
      class="cassie-query-schema-tree"
      data-testid="query-schema-tree"
      aria-label="Schema objects"
    >
      <header class="cassie-query-schema-tree-header">
        <Text size="sm" weight="semibold">
          Schema browser
        </Text>
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
            onClick={clearQuery}
          >
            <XIcon size={12} />
          </button>
        </div>
      </header>
      <div
        class="cassie-query-schema-tree-body"
        role="list"
        aria-label="Schema sections"
        data-schema-mode={schemaMode()}
      >
        <div
          class="cassie-query-schema-tree-body-root"
          data-testid="query-schema-tree-normal"
          data-schema-panel="normal"
        >
          <For each={getSchema} by={(database) => database.id}>
            {(database) => (
              <QuerySchemaTreeDatabase
                database={database}
                selectedItemId={getSelectedItemId}
                onSelectItem={onSelectItem}
              />
            )}
          </For>
        </div>

        <p class="cassie-query-schema-empty-search">No matches for “{query()}”.</p>

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
        </div>
      </div>
    </section>
  );
}
