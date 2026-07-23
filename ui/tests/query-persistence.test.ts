import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { createQueryPersistenceCoordinator } from "@/features/query/query-persistence";
import { loadQueryWorkspace } from "@/features/query/query-tabs";

const workspace = (sql: string) => ({
  version: 1 as const,
  tabs: [{ id: "tab-1", ordinal: 1, title: "Query 1", database: "postgres", sql }],
  activeTabId: "tab-1",
});

describe("query draft persistence", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    window.sessionStorage.clear();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("should_coalesce_draft_writes_after_250_ms", () => {
    // Arrange
    const coordinator = createQueryPersistenceCoordinator("alice-coalesce", vi.fn());

    // Act
    coordinator.schedule(workspace("SELECT 1"));
    coordinator.schedule(workspace("SELECT 2"));
    vi.advanceTimersByTime(249);

    // Assert
    expect(loadQueryWorkspace("alice-coalesce").tabs).toHaveLength(0);
    vi.advanceTimersByTime(1);
    expect(loadQueryWorkspace("alice-coalesce").tabs[0]?.sql).toBe("SELECT 2");
  });

  it("should_flush_the_latest_draft_during_teardown", () => {
    // Arrange
    const coordinator = createQueryPersistenceCoordinator("alice-flush", vi.fn());
    coordinator.schedule(workspace("SELECT 'latest'"));

    // Act
    const saved = coordinator.dispose();

    // Assert
    expect(saved).toBe(true);
    expect(loadQueryWorkspace("alice-flush").tabs[0]?.sql).toBe("SELECT 'latest'");
  });

  it("should_reject_oversized_drafts_without_losing_the_previous_draft", () => {
    // Arrange
    const onFailure = vi.fn();
    const coordinator = createQueryPersistenceCoordinator("alice-limit", onFailure);
    coordinator.schedule(workspace("SELECT 'safe'"));
    coordinator.flush();
    coordinator.schedule(workspace("x".repeat(1024 * 1024)));

    // Act
    const saved = coordinator.flush();

    // Assert
    expect(saved).toBe(false);
    expect(onFailure).toHaveBeenCalledOnce();
    expect(loadQueryWorkspace("alice-limit").tabs[0]?.sql).toBe("SELECT 'safe'");
  });
});
