import { For } from "@askrjs/askr/control";
import { state } from "@askrjs/askr";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
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
  const [isOpen, setIsOpen] = state(false);
  const containsSelection = () => section.items.some((item) => item.id === selectedItemId?.());
  const expanded = () => isOpen() || containsSelection();

  return (
    <Collapsible id={`schema-section-${section.id}`} open={expanded()} onOpenChange={setIsOpen}>
      <SidebarGroup
        class="cassie-query-schema-section"
        data-testid="query-schema-tree-section"
        data-section={section.id}
      >
        <SidebarGroupLabel asChild class="cassie-query-schema-section-toggle">
          <CollapsibleTrigger asChild>
            <button type="button" class="cassie-query-schema-section-toggle">
              <span class="cassie-query-schema-section-chevron" aria-hidden="true">
                <ChevronRightIcon size={13} />
              </span>
              <span class="cassie-query-schema-section-label">{section.label}</span>
              <span class="cassie-query-schema-section-count">{section.items.length}</span>
            </button>
          </CollapsibleTrigger>
        </SidebarGroupLabel>
        <CollapsibleContent asChild>
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
        </CollapsibleContent>
      </SidebarGroup>
    </Collapsible>
  );
}
