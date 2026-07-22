import type { QueryExecutionResult } from "@/features/query/query-models";

export interface QueryResultJsonProps {
  result: QueryExecutionResult;
}

export function QueryResultJson({ result }: QueryResultJsonProps) {
  const previewRows = result.rows.slice(0, 1000);
  const truncatedCount = result.rows.length - previewRows.length;
  return (
    <div class="cassie-query-json-wrap">
      {truncatedCount > 0 ? (
        <p role="status">JSON preview shows the first 1,000 of {result.rows.length} rows.</p>
      ) : null}
      <pre class="cassie-query-json">
        <code>
          {JSON.stringify(
            { command: result.command, columns: result.columns, rows: previewRows },
            null,
            2,
          )}
        </code>
      </pre>
    </div>
  );
}
