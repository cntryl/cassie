import { TableCell } from "@askrjs/themes/components";

export interface QueryResultCellProps {
  value: string | null;
}

export function QueryResultCell({ value }: QueryResultCellProps) {
  if (value === null) {
    return (
      <TableCell class="cassie-query-cell-null">
        <span class="cassie-query-cell-null-label">NULL</span>
      </TableCell>
    );
  }

  return <TableCell>{value}</TableCell>;
}
