import type { IconProps } from "@askrjs/askr/foundations/icon";
import type { JSXElement } from "@askrjs/askr/jsx-runtime";
import { For } from "@askrjs/askr/control";
import { BracesIcon, LayoutGridIcon, ListTreeIcon } from "@askrjs/lucide";
import { TabsContent, TabsList, TabsTrigger } from "@askrjs/themes/components";

export type QueryResultTab = "results" | "list" | "plan";

interface QueryResultsTabsProps {
  activeTab: () => QueryResultTab;
  onTabChange: (tab: QueryResultTab) => void;
  resultsContent: unknown;
  listContent: unknown;
  planContent: unknown;
}

const tabItems: Array<{
  id: QueryResultTab;
  label: string;
  icon: (props: IconProps) => JSXElement;
}> = [
  { id: "results", label: "Grid", icon: LayoutGridIcon },
  { id: "list", label: "JSON", icon: BracesIcon },
  { id: "plan", label: "Plan", icon: ListTreeIcon },
];

export function QueryResultsTabs({
  activeTab,
  onTabChange,
  resultsContent,
  listContent,
  planContent,
}: QueryResultsTabsProps) {
  let tabListEl: HTMLElement | null = null;

  function setTabListEl(node: HTMLElement | null) {
    tabListEl = node;
  }

  // Standard ARIA tablist keyboard pattern: only the active tab sits in the
  // normal Tab-key sequence (roving tabindex, set below), and Left/Right/
  // Home/End move focus + selection between tabs without needing Tab at all.
  function handleTabListKeyDown(event: KeyboardEvent) {
    const currentIndex = tabItems.findIndex((tab) => tab.id === activeTab());
    if (currentIndex === -1) {
      return;
    }

    const nextIndex = (() => {
      if (event.key === "ArrowRight") {
        return (currentIndex + 1) % tabItems.length;
      }
      if (event.key === "ArrowLeft") {
        return (currentIndex - 1 + tabItems.length) % tabItems.length;
      }
      if (event.key === "Home") {
        return 0;
      }
      if (event.key === "End") {
        return tabItems.length - 1;
      }

      return null;
    })();

    if (nextIndex === null) {
      return;
    }

    event.preventDefault();
    const nextTab = tabItems[nextIndex];
    onTabChange(nextTab.id);

    const nextTrigger = tabListEl?.querySelector(`[data-tab="${nextTab.id}"]`);
    if (nextTrigger instanceof HTMLElement) {
      nextTrigger.focus();
    }
  }

  // Each usage below calls activeTab() directly at its own JSX position
  // rather than hoisting a single `const active = activeTab()` — askr tracks
  // signal reads per JSX binding site, so a hoisted local is only a one-time
  // snapshot and silently stops updating on tab changes (confirmed by a
  // failing test when this was tried).
  return (
    <section
      class="cassie-query-results-tabs"
      data-testid="query-results-tabs"
      aria-label="Query results"
    >
      <TabsList
        ref={setTabListEl}
        class="cassie-query-tabs"
        aria-label="Result tab group"
        onKeyDown={handleTabListKeyDown}
      >
        <For each={tabItems} by={(tab) => tab.id}>
          {(tab) => (
            <TabsTrigger
              type="button"
              class="cassie-query-tab-trigger"
              data-testid={`query-result-tab-${tab.id}`}
              data-tab={tab.id}
              data-active={activeTab() === tab.id ? "true" : undefined}
              data-state={activeTab() === tab.id ? "active" : undefined}
              id={`query-result-tab-${tab.id}`}
              aria-controls={`query-result-panel-${tab.id}`}
              aria-selected={activeTab() === tab.id}
              tabIndex={activeTab() === tab.id ? 0 : -1}
              onClick={() => {
                onTabChange(tab.id);
              }}
            >
              <tab.icon size={14} />
              <span>{tab.label}</span>
            </TabsTrigger>
          )}
        </For>
      </TabsList>
      <div class="cassie-query-tab-content" aria-live="polite">
        {activeTab() === "results" && (
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
        {activeTab() === "list" && (
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
        {activeTab() === "plan" && (
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
