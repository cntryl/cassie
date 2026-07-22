import { VirtualTable, type VirtualTableColumn } from "@askrjs/themes/components";

import type { QueryResultValue } from "@/adapters";
import type { QueryExecutionResult } from "@/features/query/query-models";
import { QueryPlaceholder } from "./query-placeholder";

export interface QueryResultTableProps {
  result: QueryExecutionResult;
}

interface ResultRow {
  index: number;
  values: QueryResultValue[];
}

function displayQueryValue(value: QueryResultValue) {
  if (value === null) {
    return "NULL";
  }
  if (typeof value === "string") return value;
  if (typeof value === "number" || typeof value === "boolean") return value.toString();
  return JSON.stringify(value);
}

export function QueryResultTable({ result }: QueryResultTableProps) {
  const headers = Array.isArray(result.columns) ? result.columns : [];
  const values = Array.isArray(result.rows) ? result.rows : [];

  if (headers.length === 0) {
    return <QueryPlaceholder title="No columns" description="This query returned no columns." />;
  }
  if (values.length === 0) {
    return <QueryPlaceholder title="No rows" description="The query returned zero rows." />;
  }

  const rows: ResultRow[] = values.map((row, index) => ({ index, values: row }));
  const columns: VirtualTableColumn<ResultRow>[] = [
    {
      id: "row-number",
      header: "#",
      width: 64,
      cellComponent: ({ row }) => <span class="cassie-query-row-number">{row.index + 1}</span>,
    },
    ...headers.map((header, columnIndex) => ({
      id: `column-${columnIndex}`,
      header,
      width: 180,
      cellComponent: ({ row }: { row: ResultRow }) => {
        const value = row.values[columnIndex] ?? null;
        return (
          <span
            class={value === null ? "cassie-query-cell-null" : undefined}
            title={displayQueryValue(value)}
          >
            {displayQueryValue(value)}
          </span>
        );
      },
    })),
  ];

  return (
    <div class="cassie-query-result-table-wrap">
      <VirtualTable
        class="cassie-query-result-table"
        aria-label={`${rows.length} query result rows`}
        rows={rows}
        columns={columns}
        getKey={(row) => row.index}
        rowHeight={36}
        headerHeight={40}
        overscan={6}
      />
    </div>
  );
}
