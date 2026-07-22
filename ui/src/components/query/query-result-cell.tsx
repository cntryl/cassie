import { TableCell } from "@askrjs/themes/components";
import type { QueryResultValue } from "@/adapters";

export interface QueryResultCellProps {
  value: QueryResultValue;
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

export function QueryResultCell({ value }: QueryResultCellProps) {
  if (value === null) {
    return (
      <TableCell class="cassie-query-cell-null">
        <span class="cassie-query-cell-null-label">NULL</span>
      </TableCell>
    );
  }

  return <TableCell>{displayQueryValue(value)}</TableCell>;
}
