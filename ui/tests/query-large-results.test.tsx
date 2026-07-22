import { afterEach, describe, expect, it } from "vite-plus/test";
import { cleanupApp, createSPA } from "@askrjs/askr/boot";

import { QueryResultJson } from "@/components/query/query-result-json";
import { QueryResultTable } from "@/components/query/query-result-table";
import type { QueryExecutionResult } from "@/features/query/query-models";

function largeResult(count: number): QueryExecutionResult {
  return {
    command: "SELECT",
    columns: ["id", "value"],
    rows: Array.from({ length: count }, (_, index) => [index, `row-${index}`]),
  };
}

async function mount(component: () => JSX.Element) {
  cleanupApp("app");
  document.body.innerHTML = '<div id="app"></div>';
  const root = document.getElementById("app");
  if (!root) throw new Error("missing app root");
  window.history.pushState({}, "", "/");
  await createSPA({ root, routes: [{ path: "/", handler: component }] });
  await new Promise<void>((resolve) => queueMicrotask(resolve));
  return root;
}

describe("large query results", () => {
  afterEach(() => cleanupApp("app"));

  it("should_render_only_a_bounded_virtual_row_window", async () => {
    // Arrange / Act
    const root = await mount(() => <QueryResultTable result={largeResult(10_000)} />);

    // Assert
    expect(root.querySelectorAll('[data-slot="virtual-table-row"]').length).toBeLessThanOrEqual(20);
    expect(root.querySelector('[aria-label="10000 query result rows"]')).toBeTruthy();
  });

  it("should_serialize_exactly_the_first_one_thousand_rows", async () => {
    // Arrange / Act
    const root = await mount(() => <QueryResultJson result={largeResult(1_005)} />);
    const code = root.querySelector("code")?.textContent ?? "";
    const preview = JSON.parse(code) as QueryExecutionResult;

    // Assert
    expect(preview.rows).toHaveLength(1_000);
    expect(preview.rows[999]).toEqual([999, "row-999"]);
    expect(root.textContent).toContain("first 1,000 of 1005 rows");
  });
});
