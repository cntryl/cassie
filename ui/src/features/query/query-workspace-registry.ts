export interface QueryWorkspaceController {
  isBusy: () => boolean;
  cancel: () => Promise<void>;
}

export class QueryWorkspaceRegistry {
  readonly #controllers = new Map<string, QueryWorkspaceController>();
  readonly #activeReporters = new Map<string, (active: boolean) => void>();

  registerController(id: string, controller: QueryWorkspaceController) {
    this.#controllers.set(id, controller);
  }

  controller(id: string) {
    return this.#controllers.get(id);
  }

  remove(id: string) {
    this.#controllers.delete(id);
    this.#activeReporters.delete(id);
  }

  registerActiveReporter(id: string, reporter: (active: boolean) => void) {
    this.#activeReporters.set(id, reporter);
  }

  activate(id: string | null) {
    for (const [tabId, reportActive] of this.#activeReporters) reportActive(tabId === id);
  }
}
