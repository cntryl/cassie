import { state } from "@askrjs/askr";
import { For, Show } from "@askrjs/askr/control";
import { CheckIcon, FileTextIcon, PencilIcon, PlusIcon, Trash2Icon, XIcon } from "@askrjs/lucide";
import {
  Button,
  Input,
  SidebarMenu,
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
  Text,
} from "@askrjs/themes/components";

import type { DatabaseCatalogEntry } from "@/features/query/database-catalog-controller";
import type { QuerySchemaItem } from "@/features/query/query-models";
import type { PersistedQueryTab } from "@/features/query/query-tabs";
import { QuerySchemaTree } from "./query-schema-tree";

interface QuerySidebarProps {
  queries: PersistedQueryTab[] | (() => PersistedQueryTab[]);
  activeQueryId: string | null | (() => string | null);
  catalogs: DatabaseCatalogEntry[] | (() => DatabaseCatalogEntry[]);
  activeDatabase?: string | (() => string | undefined);
  selectedItemId?: string | (() => string | undefined);
  onActivateQuery: (id: string) => void;
  onRenameQuery: (id: string, title: string) => void;
  onRemoveQuery: (query: PersistedQueryTab) => void;
  onNewQuery: () => void;
  onCreateDatabase: () => void;
  onSelectSchemaItem: (item: QuerySchemaItem) => void;
  onSetDatabaseExpanded: (database: string, expanded: boolean) => void;
  onSearchCatalogs: () => void;
  onRetryDatabase: (database: string) => void;
}

export function QuerySidebar({
  queries,
  activeQueryId,
  catalogs,
  activeDatabase,
  selectedItemId,
  onActivateQuery,
  onRenameQuery,
  onRemoveQuery,
  onNewQuery,
  onCreateDatabase,
  onSelectSchemaItem,
  onSetDatabaseExpanded,
  onSearchCatalogs,
  onRetryDatabase,
}: QuerySidebarProps) {
  const [selectedItems, setSelectedItems] = state<Record<string, string | undefined>>({});
  const getQueries = () => (typeof queries === "function" ? queries() : queries);
  const getActiveQueryId = () =>
    typeof activeQueryId === "function" ? activeQueryId() : activeQueryId;
  const getCurrentSelectedItemId = () => {
    const externalSelectedItemId =
      typeof selectedItemId === "function" ? selectedItemId() : selectedItemId;
    const currentActiveQueryId = getActiveQueryId();
    return (
      externalSelectedItemId ??
      (currentActiveQueryId ? selectedItems()[currentActiveQueryId] : undefined)
    );
  };
  getCurrentSelectedItemId();

  function selectSchemaItem(item: QuerySchemaItem) {
    const queryId = getActiveQueryId();
    if (queryId) setSelectedItems({ ...selectedItems(), [queryId]: item.id });
    onSelectSchemaItem(item);
  }

  return (
    <div class="cassie-query-sidebar">
      <QuerySchemaTree
        catalogs={catalogs}
        activeDatabase={activeDatabase}
        selectedItemId={getCurrentSelectedItemId}
        onSelectItem={selectSchemaItem}
        onCreateDatabase={onCreateDatabase}
        onSetDatabaseExpanded={onSetDatabaseExpanded}
        onSearchCatalogs={onSearchCatalogs}
        onRetryDatabase={onRetryDatabase}
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
        <SidebarMenu
          class="cassie-query-list-items"
          hidden={getQueries().length === 0}
          style={{ display: getQueries().length === 0 ? "none" : undefined }}
        >
          <For each={getQueries} by={(query) => query.id}>
            {(query) => (
              <SavedQueryListItem
                query={query}
                active={() => getActiveQueryId() === query.id}
                onActivate={onActivateQuery}
                onRename={onRenameQuery}
                onRemove={onRemoveQuery}
              />
            )}
          </For>
        </SidebarMenu>
      </section>
    </div>
  );
}

function SavedQueryListItem({
  query,
  active,
  onActivate,
  onRename,
  onRemove,
}: {
  query: PersistedQueryTab;
  active: boolean | (() => boolean);
  onActivate: (id: string) => void;
  onRename: (id: string, title: string) => void;
  onRemove: (query: PersistedQueryTab) => void;
}) {
  const [editing, setEditing] = state(false);
  const [draft, setDraft] = state(query.title);

  const cancel = () => {
    setDraft(query.title);
    setEditing(false);
  };
  const save = () => {
    if (!draft().trim()) return;
    onRename(query.id, draft());
    setEditing(false);
  };

  return (
    <SidebarMenuItem class="cassie-query-list-item" data-editing={editing() ? "true" : undefined}>
      <Show
        when={editing()}
        fallback={
          <>
            <QuerySelectButton query={query} active={active} onActivate={onActivate} />
            <SidebarMenuAction
              type="button"
              class="cassie-query-list-action"
              aria-label={`Rename ${query.title}`}
              title={`Rename ${query.title}`}
              onClick={() => setEditing(true)}
            >
              <PencilIcon size={14} aria-hidden="true" />
            </SidebarMenuAction>
            <SidebarMenuAction
              type="button"
              class="cassie-query-list-action"
              aria-label={`Delete ${query.title}`}
              title={`Delete ${query.title}`}
              onClick={() => onRemove(query)}
            >
              <Trash2Icon size={14} class="cassie-query-delete-icon" aria-hidden="true" />
            </SidebarMenuAction>
          </>
        }
      >
        <Input
          aria-label={`Query name for ${query.title}`}
          value={draft()}
          onInput={(event) => setDraft((event.target as HTMLInputElement).value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") save();
            if (event.key === "Escape") cancel();
          }}
        />
        <SidebarMenuAction
          type="button"
          aria-label="Save query name"
          disabled={!draft().trim()}
          onClick={save}
        >
          <CheckIcon size={14} aria-hidden="true" />
        </SidebarMenuAction>
        <SidebarMenuAction type="button" aria-label="Cancel renaming" onClick={cancel}>
          <XIcon size={14} aria-hidden="true" />
        </SidebarMenuAction>
      </Show>
    </SidebarMenuItem>
  );
}

function QuerySelectButton({
  query,
  active,
  onActivate,
}: {
  query: PersistedQueryTab;
  active: boolean | (() => boolean);
  onActivate: (id: string) => void;
}) {
  const isActive = typeof active === "function" ? active() : active;
  return (
    <SidebarMenuButton
      id={`saved-query-${query.id}`}
      class="cassie-query-list-select"
      active={isActive}
      aria-current={isActive ? "page" : undefined}
      onClick={() => onActivate(query.id)}
    >
      <FileTextIcon size={14} aria-hidden="true" />
      <span>
        <strong>{query.title}</strong>
        <small>{query.database}</small>
      </span>
    </SidebarMenuButton>
  );
}
