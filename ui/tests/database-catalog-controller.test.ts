import { describe, expect, it } from "vite-plus/test";

import { DatabaseCatalogController } from "@/features/query/database-catalog-controller";
import type { QuerySchema } from "@/features/query/query-models";

function schemaFor(database: string): QuerySchema {
  return {
    databases: [
      {
        id: database,
        label: database,
        namespaces: [],
      },
    ],
  };
}

describe("database catalog controller", () => {
  it("should_abort_in_flight_catalog_loads_when_disposed", async () => {
    // Arrange
    let signal: AbortSignal | undefined;
    const controller = new DatabaseCatalogController((_database, options) => {
      signal = options?.signal;
      return new Promise(() => undefined);
    });
    controller.insert("Database1");
    void controller.activate("Database1");
    await new Promise<void>((resolve) => queueMicrotask(resolve));
    expect(signal).toBeDefined();

    // Act
    controller.dispose();

    // Assert
    expect(signal?.aborted).toBe(true);
  });

  it("should_load_and_expand_the_active_database_before_collapsed_siblings", async () => {
    // Arrange
    const requested: string[] = [];
    const controller = new DatabaseCatalogController(async (database) => {
      requested.push(database);
      return schemaFor(database);
    });
    controller.reconcile(["Database1", "Database2"]);

    // Act
    await controller.activate("Database2");

    // Assert
    expect(requested).toEqual(["Database2"]);
    expect(controller.entry("Database1")).toMatchObject({
      expanded: false,
      status: "idle",
    });
    expect(controller.entry("Database2")).toMatchObject({
      expanded: true,
      status: "loaded",
    });
  });

  it("should_cache_a_loaded_catalog_across_repeated_expansion", async () => {
    // Arrange
    let requests = 0;
    const controller = new DatabaseCatalogController(async (database) => {
      requests += 1;
      return schemaFor(database);
    });
    controller.reconcile(["Database1"]);

    // Act
    await controller.setExpanded("Database1", true);
    await controller.setExpanded("Database1", false);
    await controller.setExpanded("Database1", true);

    // Assert
    expect(requests).toBe(1);
    expect(controller.entry("Database1")?.status).toBe("loaded");
  });

  it("should_retry_a_failed_catalog_without_discarding_other_cached_catalogs", async () => {
    // Arrange
    const attempts = new Map<string, number>();
    const controller = new DatabaseCatalogController(async (database) => {
      const attempt = (attempts.get(database) ?? 0) + 1;
      attempts.set(database, attempt);
      if (database === "Database2" && attempt === 1) {
        throw new Error("catalog unavailable");
      }
      return schemaFor(database);
    });
    controller.reconcile(["Database1", "Database2"]);
    await controller.activate("Database1");
    await controller.setExpanded("Database2", true);
    expect(controller.entry("Database2")?.status).toBe("error");

    // Act
    await controller.retry("Database2");

    // Assert
    expect(controller.entry("Database1")?.status).toBe("loaded");
    expect(controller.entry("Database2")?.status).toBe("loaded");
    expect(attempts.get("Database2")).toBe(2);
  });

  it("should_bound_concurrent_catalog_loading_for_search", async () => {
    // Arrange
    let active = 0;
    let peak = 0;
    const controller = new DatabaseCatalogController(async (database) => {
      active += 1;
      peak = Math.max(peak, active);
      await new Promise<void>((resolve) => setTimeout(resolve, 0));
      active -= 1;
      return schemaFor(database);
    });
    controller.reconcile(["db1", "db2", "db3", "db4", "db5"]);

    // Act
    await controller.loadRemaining(2);

    // Assert
    expect(peak).toBe(2);
    expect(controller.entries().every((entry) => entry.status === "loaded")).toBe(true);
  });
});
