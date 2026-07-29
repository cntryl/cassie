import { For } from "@askrjs/askr/control";
import { state } from "@askrjs/askr";
import {
  SidebarGroup,
  SidebarGroupContent,
} from "@askrjs/themes/components";
import { ChevronRightIcon, DatabaseIcon } from "@askrjs/lucide";

import type { QuerySchemaDatabase, QuerySchemaItem } from "@/features/query/query-models";
import { QuerySchemaTreeNamespace } from "./query-schema-tree-namespace";

interface QuerySchemaTreeDatabaseProps {
  database: QuerySchemaDatabase;
  selectedItemId?: () => string | undefined;
  onSelectItem: (item: QuerySchemaItem) => void;
}

export function QuerySchemaTreeDatabase({
  database,
  selectedItemId,
  onSelectItem,
}: QuerySchemaTreeDatabaseProps) {
  const [isOpen, setIsOpen] = state(true);

  return (
    <SidebarGroup
      class="cassie-query-schema-database"
      data-testid="query-schema-tree-database"
      data-database={database.id}
    >
      <button
        type="button"
        class="cassie-query-schema-database-toggle"
        aria-expanded={isOpen() ? "true" : "false"}
        data-state={isOpen() ? "open" : "closed"}
        onClick={() => setIsOpen((previous) => !previous)}
      >
        <span class="cassie-query-schema-database-chevron" aria-hidden="true">
          <ChevronRightIcon size={13} />
        </span>
        <span class="cassie-query-schema-database-icon" aria-hidden="true">
          <DatabaseIcon size={13} />
        </span>
        <span class="cassie-query-schema-database-label">{database.label}</span>
      </button>
      {isOpen() ? (
        <SidebarGroupContent class="cassie-query-schema-database-content">
          <For each={database.namespaces} by={(namespace) => namespace.id}>
            {(namespace) => (
              <QuerySchemaTreeNamespace
                namespace={namespace}
                selectedItemId={selectedItemId}
                onSelectItem={onSelectItem}
              />
            )}
          </For>
        </SidebarGroupContent>
      ) : null}
    </SidebarGroup>
  );
}
