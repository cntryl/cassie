import { For } from "@askrjs/askr/control";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
} from "@askrjs/themes/components";
import { ChevronRightIcon, FolderIcon } from "@askrjs/lucide";

import type { QuerySchemaItem, QuerySchemaNamespace } from "@/features/query/query-models";
import { QuerySchemaTreeSection } from "./query-schema-tree-section";

interface QuerySchemaTreeNamespaceProps {
  namespace: QuerySchemaNamespace;
  selectedItemId?: () => string | undefined;
  onSelectItem: (item: QuerySchemaItem) => void;
}

export function QuerySchemaTreeNamespace({
  namespace,
  selectedItemId,
  onSelectItem,
}: QuerySchemaTreeNamespaceProps) {
  const isEmpty = namespace.sections.every((section) => section.items.length === 0);

  return (
    <Collapsible defaultOpen={!isEmpty}>
      <SidebarGroup
        class="cassie-query-schema-namespace"
        data-testid="query-schema-tree-namespace"
        data-namespace={namespace.id}
        data-empty={isEmpty ? "true" : undefined}
      >
        <SidebarGroupLabel asChild class="cassie-query-schema-namespace-toggle">
          <CollapsibleTrigger>
            <span class="cassie-query-schema-namespace-chevron" aria-hidden="true">
              <ChevronRightIcon size={13} />
            </span>
            <span class="cassie-query-schema-namespace-icon" aria-hidden="true">
              <FolderIcon size={13} />
            </span>
            <span class="cassie-query-schema-namespace-label">{namespace.label}</span>
          </CollapsibleTrigger>
        </SidebarGroupLabel>
        <CollapsibleContent>
          <SidebarGroupContent class="cassie-query-schema-namespace-content">
            <For each={namespace.sections} by={(section) => section.id}>
              {(section) => (
                <QuerySchemaTreeSection
                  section={section}
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
