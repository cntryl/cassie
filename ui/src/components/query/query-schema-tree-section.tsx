import { For, Show } from "@askrjs/askr/control";
import { state } from "@askrjs/askr";
import {
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
} from "@askrjs/themes/components";
import { ChevronRightIcon } from "@askrjs/lucide";

import type { QuerySchemaItem, QuerySchemaSection } from "@/features/query/query-models";
import { QuerySchemaTreeItem } from "./query-schema-tree-item";

interface QuerySchemaTreeSectionProps {
  section: QuerySchemaSection;
  selectedItemId?: () => string | undefined;
  onSelectItem: (item: QuerySchemaItem) => void;
}

export function QuerySchemaTreeSection({
  section,
  selectedItemId,
  onSelectItem,
}: QuerySchemaTreeSectionProps) {
  const isEmpty = section.items.length === 0;
  const [isOpen, setIsOpen] = state(!isEmpty);

  return (
    <SidebarGroup
      class="cassie-query-schema-section"
      data-testid="query-schema-tree-section"
      data-section={section.id}
      data-empty={isEmpty ? "true" : undefined}
    >
      <SidebarGroupLabel asChild class="cassie-query-schema-section-toggle">
        <button
          type="button"
          class="cassie-query-schema-section-toggle"
          aria-expanded={isOpen() ? "true" : "false"}
          data-state={isOpen() ? "open" : "closed"}
          onClick={() => setIsOpen((previous) => !previous)}
        >
          <span class="cassie-query-schema-section-chevron" aria-hidden="true">
            <ChevronRightIcon size={13} />
          </span>
          <span class="cassie-query-schema-section-label">{section.label}</span>
          <span class="cassie-query-schema-section-count">{section.items.length}</span>
        </button>
      </SidebarGroupLabel>
      <Show when={isOpen()}>
        <SidebarGroupContent>
          <SidebarMenu class="cassie-query-schema-section-list" aria-label={section.label}>
            <For each={section.items} by={(item) => item.id}>
              {(item) => (
                <QuerySchemaTreeItem
                  item={item}
                  selected={() => selectedItemId?.() === item.id}
                  onSelectItem={onSelectItem}
                />
              )}
            </For>
          </SidebarMenu>
        </SidebarGroupContent>
      </Show>
    </SidebarGroup>
  );
}
