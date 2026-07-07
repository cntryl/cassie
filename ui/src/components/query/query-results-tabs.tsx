import { For } from "@askrjs/askr/control";

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
    <section class="cassie-query-results-tabs" data-testid="query-results-tabs" aria-label="Query results">
      <div class="cassie-query-tabs" role="tablist" aria-label="Result tab group">
        <For each={tabItems} by={(tab) => tab.id}>
          {(tab) => (
            <button
              type="button"
              role="tab"
              data-testid={`query-result-tab-${tab.id}`}
              data-tab={tab.id}
              aria-selected={activeTab === tab.id}
              class={`cassie-query-tab-btn ${activeTab === tab.id ? "is-active" : ""}`}
              onClick={() => {
                onTabChange(tab.id);
              }}
            >
              {tab.label}
            </button>
          )}
        </For>
      </div>
      <div class="cassie-query-tab-content" aria-live="polite">
        {activeTab === "results" && (
          <div class="cassie-query-tab-panel" data-testid="query-tab-content" data-tab-content="results">
            {resultsContent}
          </div>
        )}
        {activeTab === "list" && (
          <div class="cassie-query-tab-panel" data-testid="query-tab-content" data-tab-content="list">
            {listContent}
          </div>
        )}
        {activeTab === "plan" && (
          <div class="cassie-query-tab-panel" data-testid="query-tab-content" data-tab-content="plan">
            {planContent}
          </div>
        )}
      </div>
    </section>
  );
}
