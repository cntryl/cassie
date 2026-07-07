import { Text } from "@askrjs/themes/components";

import type { QuerySchemaObject } from "@/data/query-schema";

interface QuerySchemaTreeItemProps {
  item: QuerySchemaObject;
  selected: boolean;
  onSelectItem: (item: QuerySchemaObject) => void;
}

export function QuerySchemaTreeItem({
  item,
  selected,
  onSelectItem,
}: QuerySchemaTreeItemProps) {
  const selectedClass = selected ? "is-selected" : "";

  return (
    <li>
      <button
        type="button"
        class={`cassie-query-schema-item ${selectedClass}`}
        data-testid="query-schema-item"
        data-item-id={item.id}
        data-item-kind={item.kind}
        aria-pressed={selected}
        onClick={() => {
          onSelectItem(item);
        }}
      >
        <Text as="span" size="sm" weight="medium">
          {item.label}
        </Text>
        {item.metadata ? <small class="cassie-query-schema-item-meta">{item.metadata}</small> : null}
      </button>
    </li>
  );
}
