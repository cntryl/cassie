import { For } from "@askrjs/askr/control";
import { state } from "@askrjs/askr";
import {
  SidebarGroup,
  SidebarGroupContent,
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
  const [isOpen, setIsOpen] = state(!isEmpty);

  return (
    <SidebarGroup
      class="cassie-query-schema-namespace"
      data-testid="query-schema-tree-namespace"
      data-namespace={namespace.id}
      data-empty={isEmpty ? "true" : undefined}
    >
      <button
        type="button"
        class="cassie-query-schema-namespace-toggle"
        aria-expanded={isOpen() ? "true" : "false"}
        data-state={isOpen() ? "open" : "closed"}
        onClick={() => setIsOpen((previous) => !previous)}
      >
        <span class="cassie-query-schema-namespace-chevron" aria-hidden="true">
          <ChevronRightIcon size={13} />
        </span>
        <span class="cassie-query-schema-namespace-icon" aria-hidden="true">
          <FolderIcon size={13} />
        </span>
        <span class="cassie-query-schema-namespace-label">{namespace.label}</span>
      </button>
      {isOpen() ? (
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
      ) : null}
    </SidebarGroup>
  );
}
