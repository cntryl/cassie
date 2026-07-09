import { For } from "@askrjs/askr/control";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
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
  return (
    <Collapsible defaultOpen>
      <SidebarGroup
        class="cassie-query-schema-database"
        data-testid="query-schema-tree-database"
        data-database={database.id}
      >
        <SidebarGroupLabel asChild class="cassie-query-schema-database-toggle">
          <CollapsibleTrigger>
            <span class="cassie-query-schema-database-chevron" aria-hidden="true">
              <ChevronRightIcon size={13} />
            </span>
            <span class="cassie-query-schema-database-icon" aria-hidden="true">
              <DatabaseIcon size={13} />
            </span>
            <span class="cassie-query-schema-database-label">{database.label}</span>
          </CollapsibleTrigger>
        </SidebarGroupLabel>
        <CollapsibleContent>
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
        </CollapsibleContent>
      </SidebarGroup>
    </Collapsible>
  );
}
