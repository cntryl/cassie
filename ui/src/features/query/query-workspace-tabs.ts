import type { PersistedQueryTab } from "./query-tabs";

export const defaultQuery = "SELECT 1 AS ready;";

export function createWorkspaceTab(
  tabs: PersistedQueryTab[],
  database: string,
  requestedTitle: string,
  id: string,
): PersistedQueryTab {
  const ordinal = tabs.reduce((largest, tab) => Math.max(largest, tab.ordinal), 0) + 1;
  return {
    id,
    ordinal,
    title: requestedTitle.trim() || `Query ${ordinal}`,
    database,
    sql: defaultQuery,
  };
}

export function renameWorkspaceTab(
  tabs: PersistedQueryTab[],
  id: string,
  title: string,
): PersistedQueryTab[] {
  const nextTitle = title.trim();
  if (!nextTitle) return tabs;
  return tabs.map((tab) => (tab.id === id ? { ...tab, title: nextTitle } : tab));
}

export function updateWorkspaceTab(
  tabs: PersistedQueryTab[],
  id: string,
  update: Partial<Pick<PersistedQueryTab, "sql" | "editorSplitPercent">>,
): PersistedQueryTab[] {
  for (const tab of tabs) {
    if (tab.id === id) Object.assign(tab, update);
  }
  return tabs;
}

export function removeWorkspaceTab(
  tabs: PersistedQueryTab[],
  activeTabId: string | null,
  id: string,
): { tabs: PersistedQueryTab[]; activeTabId: string | null } {
  const removedIndex = tabs.findIndex((tab) => tab.id === id);
  const nextTabs = tabs.filter((tab) => tab.id !== id);
  const nextActiveTabId =
    activeTabId === id ? (nextTabs[Math.max(0, removedIndex - 1)]?.id ?? null) : activeTabId;
  return { tabs: nextTabs, activeTabId: nextActiveTabId };
}
