import { For } from "@askrjs/askr/control";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeaderCell,
  TableRow,
} from "@askrjs/themes/components";

import type { QueryExecutionResult } from "@/features/query/query-models";
import { QueryPlaceholder } from "./query-placeholder";
import { QueryResultCell } from "./query-result-cell";

export interface QueryResultTableProps {
  result: QueryExecutionResult;
}

export function QueryResultTable({ result }: QueryResultTableProps) {
  const columns = Array.isArray(result.columns) ? result.columns : [];
  const rows = Array.isArray(result.rows) ? result.rows : [];

  if (columns.length === 0) {
    return <QueryPlaceholder title="No columns" description="This query returned no columns." />;
  }

  return (
    <div class="cassie-query-result-table-wrap">
      <Table class="cassie-query-result-table">
        <TableHead>
          <TableRow>
            <TableHeaderCell class="cassie-query-row-number-cell" aria-hidden="true" />
            <For each={columns} by={(_, index) => index}>
              {(column) => <TableHeaderCell>{column}</TableHeaderCell>}
            </For>
          </TableRow>
        </TableHead>
        <TableBody>
          {rows.length === 0 ? (
            <TableRow>
              <TableCell colSpan={columns.length + 1} class="cassie-query-empty-result-cell">
                <QueryPlaceholder title="No rows" description="The query returned zero rows." />
              </TableCell>
            </TableRow>
          ) : null}
          <For each={rows} by={(_, index) => index}>
            {(row, index) => (
              <TableRow>
                <TableCell class="cassie-query-row-number-cell">{index() + 1}</TableCell>
                <For each={row} by={(_, cellIndex) => cellIndex}>
                  {(value) => <QueryResultCell value={value} />}
                </For>
              </TableRow>
            )}
          </For>
        </TableBody>
      </Table>
    </div>
  );
}
