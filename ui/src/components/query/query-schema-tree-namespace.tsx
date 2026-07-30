import { For, Show } from "@askrjs/askr/control";
import { state } from "@askrjs/askr";
import { SidebarGroup, SidebarGroupContent } from "@askrjs/themes/components";
import { ChevronRightIcon, FolderIcon } from "@askrjs/lucide";

import type { QuerySchemaItem, QuerySchemaNamespace } from "@/features/query/query-models";
import { QuerySchemaTreeSection } from "./query-schema-tree-section";

interface QuerySchemaTreeNamespaceProps {
  namespace: QuerySchemaNamespace;
  openByDefault: boolean;
  selectedItemId?: () => string | undefined;
  onSelectItem: (item: QuerySchemaItem) => void;
}

export function QuerySchemaTreeNamespace({
  namespace,
  openByDefault,
  selectedItemId,
  onSelectItem,
}: QuerySchemaTreeNamespaceProps) {
  const populatedSections = namespace.sections.filter((section) => section.items.length > 0);
  const [isOpen, setIsOpen] = state(openByDefault);
  const containsSelection = () =>
    populatedSections.some((section) =>
      section.items.some((item) => item.id === selectedItemId?.()),
    );
  const expanded = () => isOpen() || containsSelection();

  return (
    <SidebarGroup
      class="cassie-query-schema-namespace"
      data-testid="query-schema-tree-namespace"
      data-namespace={namespace.id}
    >
      <button
        type="button"
        class="cassie-query-schema-namespace-toggle"
        aria-expanded={expanded() ? "true" : "false"}
        data-state={expanded() ? "open" : "closed"}
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
      <Show when={expanded()}>
        <SidebarGroupContent class="cassie-query-schema-namespace-content">
          <For each={populatedSections} by={(section) => section.id}>
            {(section) => (
              <QuerySchemaTreeSection
                section={section}
                selectedItemId={selectedItemId}
                onSelectItem={onSelectItem}
              />
            )}
          </For>
        </SidebarGroupContent>
      </Show>
    </SidebarGroup>
  );
}
