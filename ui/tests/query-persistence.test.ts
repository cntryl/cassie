import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { createQueryPersistenceCoordinator } from "@/features/query/query-persistence";
import { loadQueryWorkspace, queryWorkspaceKey } from "@/features/query/query-tabs";

const workspace = (sql: string) => ({
  version: 1 as const,
  tabs: [{ id: "tab-1", ordinal: 1, title: "Query 1", database: "postgres", sql }],
  activeTabId: "tab-1",
});

describe("query draft persistence", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    window.localStorage.clear();
  });

  afterEach(() => vi.useRealTimers());

  it("should_coalesce_draft_writes_after_250_ms", () => {
    // Arrange
    const setItem = vi.spyOn(window.localStorage, "setItem");
    const coordinator = createQueryPersistenceCoordinator("alice", vi.fn());

    // Act
    coordinator.schedule(workspace("SELECT 1"));
    coordinator.schedule(workspace("SELECT 2"));
    vi.advanceTimersByTime(249);

    // Assert
    expect(setItem).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(setItem).toHaveBeenCalledTimes(1);
    expect(loadQueryWorkspace("alice").tabs[0]?.sql).toBe("SELECT 2");
  });

  it("should_flush_the_latest_draft_during_teardown", () => {
    // Arrange
    const coordinator = createQueryPersistenceCoordinator("alice", vi.fn());
    coordinator.schedule(workspace("SELECT 'latest'"));

    // Act
    const saved = coordinator.dispose();

    // Assert
    expect(saved).toBe(true);
    expect(window.localStorage.getItem(queryWorkspaceKey("alice"))).toContain("SELECT 'latest'");
  });

  it("should_report_storage_rejection_without_losing_the_memory_draft", () => {
    // Arrange
    const onFailure = vi.fn();
    vi.spyOn(window.localStorage, "setItem").mockImplementation(() => {
      throw new DOMException("Quota exceeded", "QuotaExceededError");
    });
    const coordinator = createQueryPersistenceCoordinator("alice", onFailure);
    coordinator.schedule(workspace("SELECT 'memory'"));

    // Act
    const saved = coordinator.flush();

    // Assert
    expect(saved).toBe(false);
    expect(onFailure).toHaveBeenCalledOnce();
    expect(loadQueryWorkspace("alice").tabs[0]?.sql).toBe("SELECT 'memory'");
  });
});
