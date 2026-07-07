import { Text } from "@askrjs/themes/components";
import { For } from "@askrjs/askr/control";

import type { QuerySchemaObject, QuerySchemaSection } from "@/data/query-schema";
import { QuerySchemaTreeItem } from "./query-schema-tree-item";

interface QuerySchemaTreeSectionProps {
  section: QuerySchemaSection;
  selectedItemId?: string;
  onSelectItem: (item: QuerySchemaObject) => void;
}

export function QuerySchemaTreeSection({
  section,
  selectedItemId,
  onSelectItem,
}: QuerySchemaTreeSectionProps) {
  return (
    <section class="cassie-query-schema-section" data-testid="query-schema-tree-section" data-section={section.id}>
      <Text
        as="span"
        size="sm"
        weight="semibold"
        style={{ display: "block", marginBottom: "0.25rem" }}
      >
        {section.label}
      </Text>
      <ul class="cassie-query-schema-section-list" aria-label={section.label}>
        <For each={section.items} by={(item) => item.id}>
          {(item) => (
            <QuerySchemaTreeItem
              item={item}
              selected={selectedItemId === item.id}
              onSelectItem={onSelectItem}
            />
          )}
        </For>
      </ul>
    </section>
  );
}
