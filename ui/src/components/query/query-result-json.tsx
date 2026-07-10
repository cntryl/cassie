import type { QueryExecutionResult } from "@/features/query/query-models";

export interface QueryResultJsonProps {
  result: QueryExecutionResult;
}

export function QueryResultJson({ result }: QueryResultJsonProps) {
  return (
    <pre class="cassie-query-json">
      <code>
        {JSON.stringify(
          { command: result.command, columns: result.columns, rows: result.rows },
          null,
          2,
        )}
      </code>
    </pre>
  );
}
