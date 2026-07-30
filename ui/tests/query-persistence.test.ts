import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { createQueryPersistenceCoordinator } from "@/features/query/query-persistence";
import {
  loadQueryWorkspace,
  type PersistedQueryWorkspace,
  queryWorkspaceKey,
  saveQueryWorkspace,
} from "@/features/query/query-tabs";

const workspace = (sql: string): PersistedQueryWorkspace => ({
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
    const onStatus = vi.fn();
    const coordinator = createQueryPersistenceCoordinator("alice-coalesce", onStatus);

    // Act
    coordinator.schedule(workspace("SELECT 1"));
    coordinator.schedule(workspace("SELECT 2"));
    vi.advanceTimersByTime(249);

    // Assert
    expect(loadQueryWorkspace("alice-coalesce").tabs).toHaveLength(0);
    vi.advanceTimersByTime(1);
    expect(loadQueryWorkspace("alice-coalesce").tabs[0]?.sql).toBe("SELECT 2");
    expect(onStatus).toHaveBeenCalledWith(false);
  });

  it("should_flush_the_latest_draft_during_teardown", () => {
    // Arrange
    const onStatus = vi.fn();
    const coordinator = createQueryPersistenceCoordinator("alice-flush", onStatus);
    coordinator.schedule(workspace("SELECT 'latest'"));

    // Act
    const saved = coordinator.dispose();

    // Assert
    expect(saved).toBe(true);
    expect(loadQueryWorkspace("alice-flush").tabs[0]?.sql).toBe("SELECT 'latest'");
    expect(onStatus).toHaveBeenCalledWith(false);
  });

  it("should_reject_oversized_drafts_without_losing_the_previous_draft", () => {
    // Arrange
    const onStatus = vi.fn();
    const coordinator = createQueryPersistenceCoordinator("alice-limit", onStatus);
    coordinator.schedule(workspace("SELECT 'safe'"));
    coordinator.flush();
    coordinator.schedule(workspace("x".repeat(1024 * 1024)));

    // Act
    const saved = coordinator.flush();

    // Assert
    expect(saved).toBe(false);
    expect(onStatus).toHaveBeenCalledWith(false);
    expect(onStatus).toHaveBeenCalledWith(true);
    expect(loadQueryWorkspace("alice-limit").tabs[0]?.sql).toBe("SELECT 'safe'");
  });

  it("should_ignore_flush_requests_that_do_not_match_the_pending_operation_id", () => {
    // Arrange
    const onStatus = vi.fn();
    const coordinator = createQueryPersistenceCoordinator("alice-stale", onStatus);

    coordinator.schedule(workspace("SELECT 'first'"), "op-first");
    coordinator.schedule(workspace("SELECT 'second'"), "op-second");

    // Act
    const staleSave = coordinator.flush("op-first");
    vi.advanceTimersByTime(250);

    // Assert
    expect(staleSave).toBe(true);
    expect(loadQueryWorkspace("alice-stale").tabs).toHaveLength(0);

    // Act
    const latestSave = coordinator.flush();

    // Assert
    expect(latestSave).toBe(true);
    expect(loadQueryWorkspace("alice-stale").tabs[0]?.sql).toBe("SELECT 'second'");
    expect(onStatus).toHaveBeenCalledWith(false);
  });

  it("should_report_success_after_a_later_failure_is_recovered", () => {
    // Arrange
    const onStatus = vi.fn();
    const coordinator = createQueryPersistenceCoordinator("alice-recover", onStatus);

    coordinator.schedule(workspace("SELECT 'safe'"));
    expect(coordinator.flush()).toBe(true);
    coordinator.schedule(workspace("x".repeat(1024 * 1024 + 1)));

    // Act
    const failedSave = coordinator.flush();
    coordinator.schedule(workspace("SELECT 42"));
    const recoveredSave = coordinator.flush();

    // Assert
    expect(failedSave).toBe(false);
    expect(recoveredSave).toBe(true);
    expect(onStatus).toHaveBeenLastCalledWith(false);
    expect(loadQueryWorkspace("alice-recover").tabs[0]?.sql).toBe("SELECT 42");
  });

  it("should_accept_old_tabs_and_sanitize_restored_editor_split_percentages", () => {
    // Arrange
    const tabs = [
      { ...workspace("old").tabs[0] },
      { ...workspace("low").tabs[0], id: "low", editorSplitPercent: 12 },
      { ...workspace("high").tabs[0], id: "high", editorSplitPercent: 95 },
      { ...workspace("invalid").tabs[0], id: "invalid", editorSplitPercent: null },
    ];
    window.sessionStorage.setItem(
      queryWorkspaceKey("alice-splits"),
      JSON.stringify({ version: 1, tabs, activeTabId: "old" }),
    );

    // Act
    const restored = loadQueryWorkspace("alice-splits");

    // Assert
    expect(restored.tabs.map((tab) => tab.editorSplitPercent)).toEqual([
      undefined,
      30,
      80,
      undefined,
    ]);
  });

  it("should_round_trip_a_committed_editor_split_percentage", () => {
    // Arrange
    const next = workspace("SELECT 1");
    next.tabs[0] = { ...next.tabs[0], editorSplitPercent: 64 };

    // Act
    expect(saveQueryWorkspace("alice-split-save", next)).toBe(true);

    // Assert
    expect(loadQueryWorkspace("alice-split-save").tabs[0]?.editorSplitPercent).toBe(64);
  });
});
