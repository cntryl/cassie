export interface PersistedQueryTab {
  id: string;
  ordinal: number;
  title: string;
  database: string;
  sql: string;
}

export interface PersistedQueryWorkspace {
  version: 1;
  tabs: PersistedQueryTab[];
  activeTabId: string | null;
}

const emptyWorkspace = (): PersistedQueryWorkspace => ({ version: 1, tabs: [], activeTabId: null });
const memoryStorage = new Map<string, string>();

function readStoredValue(key: string) {
  try {
    return globalThis.localStorage?.getItem(key) ?? memoryStorage.get(key) ?? null;
  } catch {
    return memoryStorage.get(key) ?? null;
  }
}

export function queryWorkspaceKey(user: string) {
  return `cassie.query-workspace.v1:${user}`;
}

export function loadQueryWorkspace(user: string): PersistedQueryWorkspace {
  try {
    const value = JSON.parse(readStoredValue(queryWorkspaceKey(user)) ?? "null");
    if (value?.version !== 1 || !Array.isArray(value.tabs)) return emptyWorkspace();
    const tabs = value.tabs.filter(
      (tab: Partial<PersistedQueryTab>) =>
        typeof tab.id === "string" &&
        typeof tab.ordinal === "number" &&
        typeof tab.title === "string" &&
        typeof tab.database === "string" &&
        typeof tab.sql === "string",
    );
    return {
      version: 1,
      tabs,
      activeTabId: tabs.some((tab: PersistedQueryTab) => tab.id === value.activeTabId)
        ? value.activeTabId
        : (tabs[0]?.id ?? null),
    };
  } catch {
    return emptyWorkspace();
  }
}

export function saveQueryWorkspace(user: string, workspace: PersistedQueryWorkspace) {
  const value = JSON.stringify(workspace);
  memoryStorage.set(queryWorkspaceKey(user), value);
  try {
    globalThis.localStorage?.setItem(queryWorkspaceKey(user), value);
  } catch {
    // The in-memory mirror keeps non-browser tests functional.
  }
}
