import { state } from "@askrjs/askr";
import { For } from "@askrjs/askr/control";
import { SidebarMenuItem, SidebarMenuButton } from "@askrjs/themes/components";
import type { IconProps } from "@askrjs/askr/foundations/icon";
import type { JSXElement } from "@askrjs/askr/jsx-runtime";
import {
  ChevronRightIcon,
  ColumnsIcon,
  EyeIcon,
  FunctionSquareIcon,
  KeyRoundIcon,
  TableIcon,
  TerminalSquareIcon,
} from "@askrjs/lucide";

import type { QuerySchemaItem, QuerySchemaItemKind } from "@/features/query/query-models";

interface QuerySchemaTreeItemProps {
  item: QuerySchemaItem;
  selected: boolean | (() => boolean);
  onSelectItem: (item: QuerySchemaItem) => void;
}

const KIND_ICONS: Record<QuerySchemaItemKind, (props: IconProps) => JSXElement> = {
  table: TableIcon,
  view: EyeIcon,
  index: KeyRoundIcon,
  udf: FunctionSquareIcon,
  procedure: TerminalSquareIcon,
};

const EXPANDABLE_KINDS: ReadonlySet<QuerySchemaItemKind> = new Set(["table", "view"]);

export function QuerySchemaTreeItem({ item, selected, onSelectItem }: QuerySchemaTreeItemProps) {
  const KindIcon = KIND_ICONS[item.kind];
  const fullText = item.metadata ? `${item.label} — ${item.metadata}` : item.label;
  const canExpand = EXPANDABLE_KINDS.has(item.kind) && (item.columns?.length ?? 0) > 0;
  const [expanded, setExpanded] = state(false);
  const isOpen = expanded() && canExpand;
  const isSelected = typeof selected === "function" ? selected() : selected;

  return (
    <SidebarMenuItem>
      <div
        class="cassie-query-schema-item-row"
        data-testid="query-schema-item-row"
        data-expandable={canExpand ? "true" : undefined}
      >
        {canExpand ? (
          <button
            type="button"
            class="cassie-query-schema-item-chevron"
            data-testid="query-schema-item-toggle"
            aria-label={isOpen ? `Collapse ${item.label}` : `Expand ${item.label}`}
            aria-expanded={isOpen}
            onClick={(event: MouseEvent) => {
              event.stopPropagation();
              setExpanded(!expanded());
            }}
          >
            <ChevronRightIcon size={12} />
          </button>
        ) : (
          <span class="cassie-query-schema-item-chevron-spacer" aria-hidden="true" />
        )}
        <SidebarMenuButton
          size="sm"
          active={isSelected}
          data-testid="query-schema-item"
          data-item-id={item.id}
          data-item-kind={item.kind}
          aria-current={isSelected ? "true" : undefined}
          title={fullText}
          onClick={() => {
            onSelectItem(item);
          }}
        >
          <span class="cassie-query-schema-item-icon" aria-hidden="true">
            <KindIcon size={13} />
          </span>
          <span class="cassie-query-schema-item-name">{item.name}</span>
          {item.metadata ? (
            <span class="cassie-query-schema-item-meta">{item.metadata}</span>
          ) : null}
        </SidebarMenuButton>
      </div>
      <ul
        class="cassie-query-schema-item-columns"
        data-testid="query-schema-item-columns"
        hidden={!isOpen}
      >
        <For each={item.columns ?? []} by={(column) => column.id}>
          {(column) => (
            <li
              class="cassie-query-schema-column"
              data-testid="query-schema-column"
              data-column-id={column.id}
              data-primary-key={column.primaryKey ? "true" : undefined}
            >
              <span class="cassie-query-schema-column-icon" aria-hidden="true">
                {column.primaryKey ? <KeyRoundIcon size={12} /> : <ColumnsIcon size={12} />}
              </span>
              <span class="cassie-query-schema-column-name">{column.name}</span>
              {column.dataType ? (
                <span class="cassie-query-schema-column-type">{column.dataType}</span>
              ) : null}
            </li>
          )}
        </For>
      </ul>
    </SidebarMenuItem>
  );
}
