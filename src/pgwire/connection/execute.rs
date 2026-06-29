use super::{Value, PortalSuspended, TryFrom};

pub(super) struct ExecuteBatch {
    pub columns: Vec<crate::executor::ColumnMeta>,
    pub rows: Vec<Vec<Value>>,
    pub command: String,
    pub suspended: Option<PortalSuspended>,
    pub should_write_row_description: bool,
}

pub(super) fn batch_from_query_result(
    result: crate::executor::QueryResult,
    limit: Option<i64>,
    should_write_row_description: bool,
) -> ExecuteBatch {
    batch_from_parts(
        result.columns,
        result.rows,
        result.command,
        0,
        limit,
        should_write_row_description,
    )
}

pub(super) fn batch_from_suspended(suspended: PortalSuspended, limit: Option<i64>) -> ExecuteBatch {
    let should_write_row_description =
        !suspended.row_description_sent && !suspended.columns.is_empty();
    batch_from_parts(
        suspended.columns,
        suspended.rows,
        suspended.command,
        suspended.next_row,
        limit,
        should_write_row_description,
    )
}

fn batch_from_parts(
    columns: Vec<crate::executor::ColumnMeta>,
    rows: Vec<Vec<Value>>,
    command: String,
    start: usize,
    limit: Option<i64>,
    should_write_row_description: bool,
) -> ExecuteBatch {
    let remaining = rows.len().saturating_sub(start);
    let take = limit
        .map_or(remaining, |limit| usize::try_from(limit.max(0)).unwrap_or(0))
        .min(remaining);
    let end = start.saturating_add(take);
    let batch_rows = rows[start..end].to_vec();
    let row_description_sent = !columns.is_empty();
    let suspended = (end < rows.len()).then(|| PortalSuspended {
        columns: columns.clone(),
        rows,
        command: command.clone(),
        next_row: end,
        row_description_sent,
    });

    ExecuteBatch {
        columns,
        rows: batch_rows,
        command,
        suspended,
        should_write_row_description,
    }
}
