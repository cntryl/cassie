import { For } from "@askrjs/askr/control";
import { Text } from "@askrjs/themes/components";

import type { QuerySchemaObject, QuerySchemaSection } from "@/data/query-schema";
import { QuerySchemaTreeSection } from "./query-schema-tree-section";

interface QuerySchemaTreeProps {
  schema: QuerySchemaSection[];
  selectedItemId?: string;
  onSelectItem: (item: QuerySchemaObject) => void;
}

export function QuerySchemaTree({
  schema,
  selectedItemId,
  onSelectItem,
}: QuerySchemaTreeProps) {
  return (
    <aside class="cassie-query-schema-tree" data-testid="query-schema-tree" aria-label="Schema objects">
      <header class="cassie-query-schema-tree-header">
        <Text size="sm" weight="semibold">
          Schema browser
        </Text>
      </header>
      <div class="cassie-query-schema-tree-body" role="list" aria-label="Schema sections">
        <For each={schema} by={(section) => section.id}>
          {(section) => (
            <QuerySchemaTreeSection
              section={section}
              selectedItemId={selectedItemId}
              onSelectItem={onSelectItem}
            />
          )}
        </For>
      </div>
    </aside>
  );
}
