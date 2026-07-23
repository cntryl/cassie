import { For } from "@askrjs/askr/control";
import { FileTextIcon, PlusIcon, XIcon } from "@askrjs/lucide";
import { Button, Text } from "@askrjs/themes/components";

import type { QuerySchemaDatabase, QuerySchemaItem } from "@/features/query/query-models";
import type { PersistedQueryTab } from "@/features/query/query-tabs";
import { QuerySchemaTree } from "./query-schema-tree";

interface QuerySidebarProps {
  queries: PersistedQueryTab[] | (() => PersistedQueryTab[]);
  activeQueryId: string | null | (() => string | null);
  schema: QuerySchemaDatabase[] | (() => QuerySchemaDatabase[]);
  selectedItemId?: string | (() => string | undefined);
  onActivateQuery: (id: string) => void;
  onRemoveQuery: (query: PersistedQueryTab) => void;
  onNewQuery: () => void;
  onCreateDatabase: () => void;
  onSelectSchemaItem: (item: QuerySchemaItem) => void;
}

export function QuerySidebar({
  queries,
  activeQueryId,
  schema,
  selectedItemId,
  onActivateQuery,
  onRemoveQuery,
  onNewQuery,
  onCreateDatabase,
  onSelectSchemaItem,
}: QuerySidebarProps) {
  const getQueries = () => (typeof queries === "function" ? queries() : queries);
  const getActiveQueryId = () =>
    typeof activeQueryId === "function" ? activeQueryId() : activeQueryId;

  return (
    <div class="cassie-query-sidebar">
      <QuerySchemaTree
        schema={schema}
        selectedItemId={selectedItemId}
        onSelectItem={onSelectSchemaItem}
        onCreateDatabase={onCreateDatabase}
      />
      <section class="cassie-query-list" aria-labelledby="my-queries-title">
        <header class="cassie-query-list-header">
          <Text id="my-queries-title" size="sm" weight="semibold">
            My Queries
          </Text>
          <Button
            id="new-query-button"
            type="button"
            size="icon"
            variant="ghost"
            aria-label="New query"
            title="New query"
            onPress={onNewQuery}
          >
            <PlusIcon size={14} aria-hidden="true" />
          </Button>
        </header>
        <Text
          class="cassie-query-list-empty"
          size="sm"
          tone="muted"
          hidden={getQueries().length > 0}
          style={{ display: getQueries().length > 0 ? "none" : undefined }}
        >
          No saved queries yet.
        </Text>
        <ul
          class="cassie-query-list-items"
          hidden={getQueries().length === 0}
          style={{ display: getQueries().length === 0 ? "none" : undefined }}
        >
          <For each={getQueries} by={(query) => query.id}>
            {(query) => (
              <li class="cassie-query-list-item">
                <button
                  type="button"
                  id={`saved-query-${query.id}`}
                  class="cassie-query-list-select"
                  aria-current={getActiveQueryId() === query.id ? "page" : undefined}
                  onClick={() => onActivateQuery(query.id)}
                >
                  <FileTextIcon size={14} aria-hidden="true" />
                  <span>
                    <strong>{query.title}</strong>
                    <small>{query.database}</small>
                  </span>
                </button>
                <button
                  type="button"
                  class="cassie-query-list-remove"
                  aria-label={`Remove ${query.title}`}
                  onClick={() => onRemoveQuery(query)}
                >
                  <XIcon size={13} aria-hidden="true" />
                </button>
              </li>
            )}
          </For>
        </ul>
      </section>
    </div>
  );
}
