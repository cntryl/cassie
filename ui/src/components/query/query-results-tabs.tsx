import { For } from "@askrjs/askr/control";
import { TabsContent, TabsList, TabsTrigger } from "@askrjs/themes/components";

export type QueryResultTab = "results" | "list" | "plan";

interface QueryResultsTabsProps {
  activeTab: QueryResultTab;
  onTabChange: (tab: QueryResultTab) => void;
  resultsContent: unknown;
  listContent: unknown;
  planContent: unknown;
}

const tabItems: Array<{ id: QueryResultTab; label: string }> = [
  { id: "results", label: "Results" },
  { id: "list", label: "List" },
  { id: "plan", label: "Plan" },
];

export function QueryResultsTabs({
  activeTab,
  onTabChange,
  resultsContent,
  listContent,
  planContent,
}: QueryResultsTabsProps) {
  return (
    <section
      class="cassie-query-results-tabs"
      data-testid="query-results-tabs"
      aria-label="Query results"
    >
      <TabsList class="cassie-query-tabs" aria-label="Result tab group">
        <For each={tabItems} by={(tab) => tab.id}>
          {(tab) => (
            <TabsTrigger
              type="button"
              data-testid={`query-result-tab-${tab.id}`}
              data-tab={tab.id}
              data-active={activeTab === tab.id ? "true" : undefined}
              data-state={activeTab === tab.id ? "active" : undefined}
              id={`query-result-tab-${tab.id}`}
              aria-controls={`query-result-panel-${tab.id}`}
              aria-selected={activeTab === tab.id}
              onClick={() => {
                onTabChange(tab.id);
              }}
            >
              {tab.label}
            </TabsTrigger>
          )}
        </For>
      </TabsList>
      <div class="cassie-query-tab-content" aria-live="polite">
        {activeTab === "results" && (
          <TabsContent
            class="cassie-query-tab-panel"
            id="query-result-panel-results"
            aria-labelledby="query-result-tab-results"
            data-testid="query-tab-content"
            data-tab-content="results"
          >
            {resultsContent}
          </TabsContent>
        )}
        {activeTab === "list" && (
          <TabsContent
            class="cassie-query-tab-panel"
            id="query-result-panel-list"
            aria-labelledby="query-result-tab-list"
            data-testid="query-tab-content"
            data-tab-content="list"
          >
            {listContent}
          </TabsContent>
        )}
        {activeTab === "plan" && (
          <TabsContent
            class="cassie-query-tab-panel"
            id="query-result-panel-plan"
            aria-labelledby="query-result-tab-plan"
            data-testid="query-tab-content"
            data-tab-content="plan"
          >
            {planContent}
          </TabsContent>
        )}
      </div>
    </section>
  );
}
