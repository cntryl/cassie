import { saveQueryWorkspace, type PersistedQueryWorkspace } from "./query-tabs";

export interface QueryPersistenceCoordinator {
  schedule(workspace: PersistedQueryWorkspace, operationId?: string): void;
  flush(operationId?: string): boolean;
  dispose(): boolean;
}

export function createQueryPersistenceCoordinator(
  user: string,
  onStatus: (failed: boolean) => void,
  delayMs = 250,
): QueryPersistenceCoordinator {
  let pending: { workspace: PersistedQueryWorkspace; operationId: string } | null = null;
  let timer: ReturnType<typeof setTimeout> | null = null;

  function markPersisted(failed: boolean) {
    onStatus(failed);
  }

  function clearTimer() {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  }

  function flush(operationId?: string) {
    clearTimer();
    if (pending === null) return true;
    if (operationId !== undefined && operationId !== pending.operationId) return true;

    const workspace = pending.workspace;
    pending = null;
    const saved = saveQueryWorkspace(user, workspace);
    markPersisted(!saved);
    return saved;
  }

  function schedule(workspace: PersistedQueryWorkspace, operationId = "coordinator") {
    pending = { workspace, operationId };
    markPersisted(false);
    clearTimer();
    timer = setTimeout(() => {
      flush(operationId);
    }, delayMs);
  }

  return {
    schedule,
    flush,
    dispose: flush,
  };
}
