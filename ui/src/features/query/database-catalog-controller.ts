import type { QuerySchema, QuerySchemaDatabase } from "./query-models";

export type DatabaseCatalogStatus = "idle" | "loading" | "loaded" | "error";

export interface DatabaseCatalogEntry {
  canonicalName: string;
  name: string;
  status: DatabaseCatalogStatus;
  expanded: boolean;
  database?: QuerySchemaDatabase;
  error?: unknown;
}

type DatabaseCatalogLoader = (
  database: string,
  options?: { signal?: AbortSignal },
) => Promise<QuerySchema>;

function canonicalName(name: string) {
  return name.trim().toLocaleLowerCase();
}

function emptyDatabase(name: string): QuerySchemaDatabase {
  return {
    id: name,
    label: name,
    namespaces: [],
  };
}

export class DatabaseCatalogController {
  readonly #entries = new Map<string, DatabaseCatalogEntry>();
  readonly #pending = new Map<string, Promise<void>>();
  readonly #abortControllers = new Set<AbortController>();
  readonly #load: DatabaseCatalogLoader;
  readonly #notify: () => void;
  readonly #invalidate: (database: string) => void;
  #disposed = false;

  constructor(
    load: DatabaseCatalogLoader,
    notify: () => void = () => {},
    invalidate: (database: string) => void = () => {},
  ) {
    this.#load = load;
    this.#notify = notify;
    this.#invalidate = invalidate;
  }

  entries(): DatabaseCatalogEntry[] {
    return [...this.#entries.values()].sort((left, right) =>
      left.name.localeCompare(right.name, undefined, { sensitivity: "base" }),
    );
  }

  entry(name: string): DatabaseCatalogEntry | undefined {
    return this.#entries.get(canonicalName(name));
  }

  reconcile(names: string[]) {
    let changed = false;
    for (const rawName of names) {
      const name = rawName.trim();
      const key = canonicalName(name);
      if (!key) continue;
      const current = this.#entries.get(key);
      if (!current) {
        this.#entries.set(key, {
          canonicalName: key,
          name,
          status: "idle",
          expanded: false,
        });
        changed = true;
      } else if (current.name !== name && current.status !== "loaded") {
        this.#entries.set(key, { ...current, name });
        changed = true;
      }
    }
    if (changed) this.#changed();
  }

  insert(name: string) {
    this.reconcile([name]);
  }

  async activate(name: string) {
    await this.setExpanded(name, true);
  }

  async setExpanded(name: string, expanded: boolean) {
    this.insert(name);
    const entry = this.entry(name);
    if (!entry) return;
    if (entry.expanded !== expanded) {
      this.#entries.set(entry.canonicalName, { ...entry, expanded });
      this.#changed();
    }
    if (expanded) {
      const current = this.entry(name);
      if (current) await this.#ensureLoaded(current, false);
    }
  }

  async retry(name: string) {
    const entry = this.entry(name);
    if (!entry) return;
    await this.#ensureLoaded(entry, true);
  }

  async refresh(name: string) {
    this.insert(name);
    const entry = this.entry(name);
    if (!entry) return;
    this.#invalidate(entry.name);
    this.#entries.set(entry.canonicalName, {
      canonicalName: entry.canonicalName,
      name: entry.name,
      status: "idle",
      expanded: entry.expanded,
    });
    this.#changed();
    const current = this.entry(name);
    if (current) await this.#ensureLoaded(current, true);
  }

  async loadRemaining(concurrency = 3) {
    const queue = this.entries().filter((entry) => entry.status === "idle");
    if (queue.length === 0) return;
    const workerCount = Math.max(1, Math.min(Math.floor(concurrency), queue.length));
    let index = 0;
    const worker = async () => {
      while (index < queue.length) {
        const entry = queue[index];
        index += 1;
        if (entry) await this.#ensureLoaded(entry, false);
      }
    };
    await Promise.all(Array.from({ length: workerCount }, worker));
  }

  dispose() {
    this.#disposed = true;
    for (const controller of this.#abortControllers) controller.abort();
    this.#abortControllers.clear();
    this.#pending.clear();
  }

  async #ensureLoaded(entry: DatabaseCatalogEntry, retry: boolean) {
    const pending = this.#pending.get(entry.canonicalName);
    if (pending) {
      await pending;
      return;
    }
    if (entry.status === "loaded" || (entry.status === "error" && !retry)) return;

    const controller = new AbortController();
    this.#abortControllers.add(controller);
    this.#entries.set(entry.canonicalName, {
      ...entry,
      status: "loading",
      error: undefined,
    });
    this.#changed();
    const request = this.#load(entry.name, { signal: controller.signal })
      .then((schema) => {
        if (this.#disposed) return;
        const database =
          schema.databases.find((database) => canonicalName(database.id) === entry.canonicalName) ??
          emptyDatabase(entry.name);
        const current = this.#entries.get(entry.canonicalName);
        if (!current) return;
        this.#entries.set(entry.canonicalName, {
          ...current,
          name: database.label,
          status: "loaded",
          database,
          error: undefined,
        });
      })
      .catch((error: unknown) => {
        if (this.#disposed || controller.signal.aborted) return;
        const current = this.#entries.get(entry.canonicalName);
        if (!current) return;
        this.#entries.set(entry.canonicalName, {
          ...current,
          status: "error",
          error,
        });
      })
      .finally(() => {
        this.#abortControllers.delete(controller);
        this.#pending.delete(entry.canonicalName);
        if (!this.#disposed) this.#changed();
      });
    this.#pending.set(entry.canonicalName, request);
    await request;
  }

  #changed() {
    if (!this.#disposed) this.#notify();
  }
}
