import { saveQueryWorkspace, type PersistedQueryWorkspace } from "./query-tabs";

export interface QueryPersistenceCoordinator {
  schedule(workspace: PersistedQueryWorkspace): void;
  flush(): boolean;
  dispose(): boolean;
}

export function createQueryPersistenceCoordinator(
  user: string,
  onFailure: () => void,
  delayMs = 250,
): QueryPersistenceCoordinator {
  let pending: PersistedQueryWorkspace | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;

  function clearTimer() {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  }

  function flush() {
    clearTimer();
    if (pending === null) return true;
    const workspace = pending;
    pending = null;
    const saved = saveQueryWorkspace(user, workspace);
    if (!saved) onFailure();
    return saved;
  }

  return {
    schedule(workspace) {
      pending = workspace;
      clearTimer();
      timer = setTimeout(flush, delayMs);
    },
    flush,
    dispose: flush,
  };
}
