export interface QueryWorkspaceController {
  isBusy: () => boolean;
  cancel: () => Promise<void>;
}

export class QueryWorkspaceRegistry {
  readonly #controllers = new Map<string, QueryWorkspaceController>();

  registerController(id: string, controller: QueryWorkspaceController) {
    this.#controllers.set(id, controller);
    return () => {
      if (this.#controllers.get(id) === controller) {
        this.#controllers.delete(id);
      }
    };
  }

  controller(id: string) {
    return this.#controllers.get(id);
  }

  remove(id: string) {
    this.#controllers.delete(id);
  }
}
