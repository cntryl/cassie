import { describe, expect, it, vi } from "vite-plus/test";

import { QueryWorkspaceRegistry } from "@/features/query/query-workspace-registry";
import {
  createWorkspaceTab,
  removeWorkspaceTab,
  renameWorkspaceTab,
  updateWorkspaceTab,
} from "@/features/query/query-workspace-tabs";
import type { PersistedQueryTab } from "@/features/query/query-tabs";

const firstTab: PersistedQueryTab = {
  id: "query-1",
  ordinal: 1,
  title: "Query 1",
  database: "cassie",
  sql: "SELECT 1;",
};

describe("query workspace orchestration", () => {
  it("should_preserve_tab_identity_given_workspace_state_updates", () => {
    // Arrange
    const tabs = [firstTab];

    // Act
    const renamed = renameWorkspaceTab(tabs, firstTab.id, "  Renamed  ");
    const updated = updateWorkspaceTab(renamed, firstTab.id, {
      sql: "SELECT 2;",
      editorSplitPercent: 61,
    });

    // Assert
    expect(updated).toEqual([
      { ...firstTab, title: "Renamed", sql: "SELECT 2;", editorSplitPercent: 61 },
    ]);
    expect(updated[0]?.id).toBe(firstTab.id);
    expect(updated).toBe(renamed);
  });

  it("should_leave_tabs_unchanged_given_a_blank_rename", () => {
    // Arrange
    const tabs = [firstTab];

    // Act
    const renamed = renameWorkspaceTab(tabs, firstTab.id, "   ");

    // Assert
    expect(renamed).toBe(tabs);
    expect(renamed[0]?.title).toBe(firstTab.title);
  });

  it("should_select_the_previous_tab_given_the_active_workspace_is_removed", () => {
    // Arrange
    const second = createWorkspaceTab([firstTab], "analytics", "", "query-2");
    const third = createWorkspaceTab([firstTab, second], "warehouse", "Third", "query-3");

    // Act
    const result = removeWorkspaceTab([firstTab, second, third], second.id, second.id);

    // Assert
    expect(result.tabs.map((tab) => tab.id)).toEqual([firstTab.id, third.id]);
    expect(result.activeTabId).toBe(firstTab.id);
  });

  it("should_report_only_the_active_registered_workspace", () => {
    // Arrange
    const registry = new QueryWorkspaceRegistry();
    const firstReporter = vi.fn();
    const secondReporter = vi.fn();
    registry.registerActiveReporter("query-1", firstReporter);
    registry.registerActiveReporter("query-2", secondReporter);

    // Act
    registry.activate("query-2");

    // Assert
    expect(firstReporter).toHaveBeenCalledWith(false);
    expect(secondReporter).toHaveBeenCalledWith(true);
  });
});
