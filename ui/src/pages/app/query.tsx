import { state } from "@askrjs/askr";
import { Text } from "@askrjs/themes/components";
import { SquareTerminalIcon } from "@askrjs/lucide";

import type { QuerySchemaObject } from "@/data/query-schema";
import { querySchema } from "@/data/query-schema";
import { QuerySchemaTree } from "@/components/query/query-schema-tree";
import { QueryEditorPanel } from "@/components/query/query-editor-panel";
import { QueryResultTab, QueryResultsTabs } from "@/components/query/query-results-tabs";
import { ResizableSplit } from "@/components/query/resizable-split";
import type { MonacoCompletionItem } from "@/components/query/monaco-sql-editor";

interface QueryPageProps {
  onSchemaItemSelect?: (item: QuerySchemaObject) => void;
  onActiveTabChange?: (tab: QueryResultTab) => void;
  onRun?: (query: string) => void;
  onStop?: () => void;
}

const defaultQuery = "SELECT id, name\nFROM documents\nLIMIT 10;";

export default function QueryPage({
  onSchemaItemSelect,
  onActiveTabChange,
  onRun,
  onStop,
}: QueryPageProps) {
  const [query, setQuery] = state(defaultQuery);
  const [activeTab, setActiveTab] = state<QueryResultTab>("results");
  const [isRunning, setIsRunning] = state(false);
  const [selectedItemId, setSelectedItemId] = state<string | null>(null);
  const [schemaWidth, setSchemaWidth] = state(30);
  const [editorHeight, setEditorHeight] = state(62);

  function handleSchemaSelection(item: QuerySchemaObject) {
    setSelectedItemId(item.id);
    setQuery(`SELECT * FROM ${item.label};`);
    onSchemaItemSelect?.(item);
  }

  function handleFormatQuery() {
    setQuery(query().trim());
  }

  function handleValidateQuery() {
    setQuery(`${query().trim()}\n`);
  }

  function handleExplainQuery() {
    setQuery(query().trim());
  }

  function handlePlayQuery() {
    if (isRunning()) {
      return;
    }

    setIsRunning(true);
    onRun?.(query());
  }

  function handleStopQuery() {
    if (!isRunning()) {
      return;
    }

    setIsRunning(false);
    onStop?.();
  }

  function handleTabChange(tab: QueryResultTab) {
    if (activeTab() === tab) {
      return;
    }

    setActiveTab(tab);
    onActiveTabChange?.(tab);
  }

  const completionItems: MonacoCompletionItem[] = querySchema.flatMap((section) =>
    section.items.map((item) => ({
      label: item.label,
      insertText: item.label,
      detail: `${item.kind} · ${item.metadata ?? "catalog item"}`,
    })),
  );

  const resultPlaceholder = "Query execution is intentionally disabled in this slice.";

  return (
    <main
      class="cassie-admin-page cassie-query-page"
      data-slot="main"
      data-query-page="true"
      data-testid="query-page"
      id="main-content"
      tabindex={-1}
      aria-labelledby="cassie-admin-page-title"
    >
      <section class="cassie-admin-page-header" aria-label="Query page">
        <div class="cassie-admin-page-icon" aria-hidden="true">
          <SquareTerminalIcon size={20} />
        </div>
        <div class="cassie-admin-page-title-group">
          <Text
            as="p"
            class="cassie-admin-page-kicker"
            size="sm"
            weight="semibold"
            transform="uppercase"
          >
            Cassie
          </Text>
          <h1 id="cassie-admin-page-title">Query</h1>
          <p>SQL and document query workflows.</p>
        </div>
      </section>

      <section class="cassie-query-workspace" aria-label="Query workspace">
        <ResizableSplit
          orientation="horizontal"
          initialSize={schemaWidth()}
          min={18}
          max={40}
          onResize={(size) => setSchemaWidth(size)}
          first={
            <QuerySchemaTree
              schema={querySchema}
              selectedItemId={selectedItemId() ?? undefined}
              onSelectItem={handleSchemaSelection}
            />
          }
          second={
            <ResizableSplit
              orientation="vertical"
              initialSize={editorHeight()}
              min={30}
              max={80}
              onResize={(size) => setEditorHeight(size)}
              first={
                <QueryEditorPanel
                  query={query()}
                  onQueryChange={setQuery}
                  isRunning={isRunning()}
                  onFormat={handleFormatQuery}
                  onValidate={handleValidateQuery}
                  onExplain={handleExplainQuery}
                  onPlay={handlePlayQuery}
                  onStop={handleStopQuery}
                  completionItems={completionItems}
                />
              }
              second={
                <QueryResultsTabs
                  activeTab={activeTab()}
                  onTabChange={handleTabChange}
                  resultsContent={
                    <section
                      class="cassie-query-results-placeholder"
                      aria-label="Query results not implemented"
                    >
                      <h3>Results</h3>
                      <p>{resultPlaceholder}</p>
                    </section>
                  }
                  listContent={
                    <section class="cassie-query-results-placeholder" aria-label="List view not implemented">
                      <h3>List</h3>
                      <p>{resultPlaceholder}</p>
                    </section>
                  }
                  planContent={
                    <section class="cassie-query-results-placeholder" aria-label="Plan view not implemented">
                      <h3>Plan</h3>
                      <p>{resultPlaceholder}</p>
                    </section>
                  }
                />
              }
            />
          }
        />
      </section>
    </main>
  );
}
