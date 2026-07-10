import type { QueryExecutionResult } from "@/features/query/query-models";

export interface QueryExecutionSummaryProps {
  result: QueryExecutionResult | null;
}

const NBSP = " ";

export function QueryExecutionSummary({ result }: QueryExecutionSummaryProps) {
  const rowCount = result && Array.isArray(result.rows) ? result.rows.length : 0;
  const columnCount = result && Array.isArray(result.columns) ? result.columns.length : 0;
  const rowText = `${rowCount} row${rowCount === 1 ? "" : "s"}`;
  const columnText = `${columnCount} column${columnCount === 1 ? "" : "s"}`;

  // Always render this at its full size — result flips between the execute
  // and explain mutations depending on which tab is active (see query.tsx's
  // activeExecution), so returning null here when the *other* action hasn't
  // run yet collapsed this block to zero height purely from switching tabs,
  // shifting everything below it. Reserve the same space and just hide the
  // (stale/inapplicable) content instead. A non-breaking space (rather than
  // "") keeps each line's rendered height identical whether or not there's a
  // command to show — an empty string collapses the line height, undoing
  // the point of reserving the space in the first place.
  return (
    <section
      class="cassie-query-execution-summary"
      aria-label="Execution summary"
      data-empty={result ? undefined : "true"}
    >
      <p class="cassie-query-execution-summary-command">
        <strong>Command</strong>
        <span>{result?.command ?? NBSP}</span>
      </p>
      <p class="cassie-query-execution-summary-meta">
        {result ? `${rowText} · ${columnText}` : NBSP}
      </p>
    </section>
  );
}
