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
const failedStorageKeys = new Set<string>();
const MAX_PERSISTED_WORKSPACE_CHARS = 1024 * 1024;
const MAX_PERSISTED_TABS = 32;
const MAX_PERSISTED_SQL_CHARS = 256 * 1024;

function browserStorage(): Storage | null {
  if (typeof window === "undefined") return null;

  try {
    return window.sessionStorage;
  } catch {
    return null;
  }
}

function readStoredValue(key: string) {
  if (failedStorageKeys.has(key)) return memoryStorage.get(key) ?? null;

  const storage = browserStorage();
  if (storage === null) return memoryStorage.get(key) ?? null;

  try {
    return storage.getItem(key);
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
    const tabs = value.tabs
      .slice(0, MAX_PERSISTED_TABS)
      .filter(
        (tab: Partial<PersistedQueryTab>) =>
          typeof tab.id === "string" &&
          typeof tab.ordinal === "number" &&
          typeof tab.title === "string" &&
          typeof tab.database === "string" &&
          typeof tab.sql === "string" &&
          tab.sql.length <= MAX_PERSISTED_SQL_CHARS,
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
  const key = queryWorkspaceKey(user);
  const value = JSON.stringify(workspace);
  if (value.length > MAX_PERSISTED_WORKSPACE_CHARS) return false;
  memoryStorage.set(key, value);
  const storage = browserStorage();
  if (storage === null) return true;

  try {
    storage.setItem(key, value);
    failedStorageKeys.delete(key);
    return true;
  } catch {
    failedStorageKeys.add(key);
    return false;
  }
}

export function clearQueryWorkspace(user: string) {
  const key = queryWorkspaceKey(user);
  memoryStorage.delete(key);
  failedStorageKeys.delete(key);
  const storage = browserStorage();
  if (storage === null) return;

  try {
    storage.removeItem(key);
  } catch {
    // The in-memory copy is still cleared when browser storage is unavailable.
  }
}
