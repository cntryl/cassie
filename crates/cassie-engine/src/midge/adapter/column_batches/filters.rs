use super::{ColumnBatchFieldSummary, ColumnBatchRow, RowFilter};

pub(super) fn column_batch_row_matches(row: &ColumnBatchRow, filter: Option<&RowFilter>) -> bool {
    let Some(filter) = filter else { return true };
    row.values
        .iter()
        .find(|(field, _)| {
            crate::sql::ColumnIdentifierPath::matches_stored_field(&filter.field, field)
        })
        .is_some_and(|(_, value)| value == &filter.value)
}

pub(super) fn column_batch_summary_supports_ordering(summary: &ColumnBatchFieldSummary) -> bool {
    summary.min.iter().chain(summary.max.iter()).all(|value| {
        !matches!(
            value,
            crate::types::Value::Vector(_) | crate::types::Value::Json(_)
        )
    })
}
