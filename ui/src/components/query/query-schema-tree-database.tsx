import { For } from "@askrjs/askr/control";
import {
  Button,
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
  SidebarGroup,
  SidebarGroupContent,
  Text,
} from "@askrjs/themes/components";
import { ChevronRightIcon, DatabaseIcon } from "@askrjs/lucide";

import type { DatabaseCatalogEntry } from "@/features/query/database-catalog-controller";
import type { QuerySchemaDatabase, QuerySchemaItem } from "@/features/query/query-models";
import { apiErrorMessage } from "@/shared/errors/api";
import { QuerySchemaTreeNamespace } from "./query-schema-tree-namespace";

interface QuerySchemaTreeDatabaseProps {
  catalog: DatabaseCatalogEntry;
  active: boolean;
  selectedItemId?: () => string | undefined;
  onSelectItem: (item: QuerySchemaItem) => void;
  onSetExpanded: (database: string, expanded: boolean) => void;
  onRetry: (database: string) => void;
}

export function QuerySchemaTreeDatabase({
  catalog,
  active,
  selectedItemId,
  onSelectItem,
  onSetExpanded,
  onRetry,
}: QuerySchemaTreeDatabaseProps) {
  const database =
    catalog.database ??
    ({
      id: catalog.name,
      label: catalog.name,
      namespaces: [],
    } satisfies QuerySchemaDatabase);
  const populatedNamespaces = database.namespaces.filter((namespace) =>
    namespace.sections.some((section) => section.items.length > 0),
  );

  return (
    <Collapsible
      id={`database-${catalog.canonicalName}`}
      open={catalog.expanded}
      onOpenChange={(expanded) => onSetExpanded(catalog.name, expanded)}
    >
      <SidebarGroup
        class="cassie-query-schema-database"
        data-testid="query-schema-tree-database"
        data-database={database.id}
        data-load-state={catalog.status}
      >
        <CollapsibleTrigger asChild>
          <button type="button" class="cassie-query-schema-database-toggle">
            <span class="cassie-query-schema-database-chevron" aria-hidden="true">
              <ChevronRightIcon size={13} />
            </span>
            <span class="cassie-query-schema-database-icon" aria-hidden="true">
              <DatabaseIcon size={13} />
            </span>
            <span class="cassie-query-schema-database-label">{database.label}</span>
          </button>
        </CollapsibleTrigger>
        <CollapsibleContent asChild>
          <SidebarGroupContent class="cassie-query-schema-database-content">
            {catalog.status === "idle" || catalog.status === "loading" ? (
              <Text
                class="cassie-query-schema-database-status"
                size="sm"
                tone="muted"
                role="status"
              >
                Loading catalog…
              </Text>
            ) : null}
            {catalog.status === "error" ? (
              <div class="cassie-query-schema-database-status" role="alert">
                <Text size="sm" tone="muted">
                  {apiErrorMessage(catalog.error)}
                </Text>
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  onPress={() => onRetry(catalog.name)}
                >
                  Retry
                </Button>
              </div>
            ) : null}
            {catalog.status === "loaded" && populatedNamespaces.length === 0 ? (
              <Text class="cassie-query-schema-database-status" size="sm" tone="muted">
                No schema objects.
              </Text>
            ) : null}
            <For each={populatedNamespaces} by={(namespace) => namespace.id}>
              {(namespace) => (
                <QuerySchemaTreeNamespace
                  namespace={namespace}
                  openByDefault={active}
                  selectedItemId={selectedItemId}
                  onSelectItem={onSelectItem}
                />
              )}
            </For>
          </SidebarGroupContent>
        </CollapsibleContent>
      </SidebarGroup>
    </Collapsible>
  );
}
