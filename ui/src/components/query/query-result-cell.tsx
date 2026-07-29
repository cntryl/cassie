import { TableCell } from "@askrjs/themes/components";
import type { QueryResultValue } from "@/adapters";

export interface QueryResultCellProps {
  value: QueryResultValue;
  title?: string;
}

function displayQueryValue(value: Exclude<QueryResultValue, null>) {
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }

  return JSON.stringify(value);
}

export function QueryResultCell({ value, title }: QueryResultCellProps) {
  const computedTitle = title ?? (value === null ? "NULL" : displayQueryValue(value));
  if (value === null) {
    return (
      <TableCell class="cassie-query-cell cassie-query-cell-null" title="NULL">
        <span class="cassie-query-cell-null-label">NULL</span>
      </TableCell>
    );
  }

  return (
    <TableCell class="cassie-query-cell" title={computedTitle}>
      {displayQueryValue(value)}
    </TableCell>
  );
}
