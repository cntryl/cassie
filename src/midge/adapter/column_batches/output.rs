use std::mem::size_of;

use super::{CassieError, ColumnBatchRow, DocumentRef};
use crate::runtime::QueryMemoryReservation;

const OBJECT_ENTRY_OVERHEAD: usize = 4 * size_of::<usize>();

pub(super) fn project_column_batch_document(
    memory: Option<&mut QueryMemoryReservation>,
    row: ColumnBatchRow,
    fields: &[String],
) -> Result<DocumentRef, CassieError> {
    let retained_bytes = projected_document_retained_bytes(&row, fields)?;
    reserve_before_projecting(memory, retained_bytes, || {
        let payload = project_column_batch_row(&row, fields);
        DocumentRef {
            id: row.row_id,
            payload,
        }
    })
}

fn reserve_before_projecting<T>(
    memory: Option<&mut QueryMemoryReservation>,
    retained_bytes: usize,
    project: impl FnOnce() -> T,
) -> Result<T, CassieError> {
    if let Some(memory) = memory {
        memory.try_grow(retained_bytes)?;
    }
    Ok(project())
}

fn projected_document_retained_bytes(
    row: &ColumnBatchRow,
    fields: &[String],
) -> Result<usize, CassieError> {
    fields.iter().try_fold(
        checked_add(
            size_of::<DocumentRef>(),
            checked_add(row.row_id.len(), size_of::<serde_json::Value>())?,
        )?,
        |bytes, field| {
            if field.eq_ignore_ascii_case("id") || field.eq_ignore_ascii_case("_id") {
                return Ok(bytes);
            }
            let value = projected_value(row, field);
            checked_add(
                bytes,
                checked_add(
                    size_of::<String>().saturating_add(OBJECT_ENTRY_OVERHEAD),
                    checked_add(field.len(), json_retained_bytes(value)?)?,
                )?,
            )
        },
    )
}

fn project_column_batch_row(row: &ColumnBatchRow, fields: &[String]) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    for field in fields {
        if field.eq_ignore_ascii_case("id") || field.eq_ignore_ascii_case("_id") {
            continue;
        }
        object.insert(field.clone(), projected_value(row, field).clone());
    }
    serde_json::Value::Object(object)
}

fn projected_value<'a>(row: &'a ColumnBatchRow, field: &str) -> &'a serde_json::Value {
    static NULL: serde_json::Value = serde_json::Value::Null;
    row.values
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(field))
        .map_or(&NULL, |(_, value)| value)
}

fn json_retained_bytes(value: &serde_json::Value) -> Result<usize, CassieError> {
    let inline = size_of::<serde_json::Value>();
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            Ok(inline)
        }
        serde_json::Value::String(value) => checked_add(inline, value.len()),
        serde_json::Value::Array(values) => values.iter().try_fold(inline, |bytes, value| {
            checked_add(bytes, json_retained_bytes(value)?)
        }),
        serde_json::Value::Object(values) => {
            values.iter().try_fold(inline, |bytes, (key, value)| {
                checked_add(
                    bytes,
                    checked_add(
                        size_of::<String>().saturating_add(OBJECT_ENTRY_OVERHEAD),
                        checked_add(key.len(), json_retained_bytes(value)?)?,
                    )?,
                )
            })
        }
    }
}

fn checked_add(left: usize, right: usize) -> Result<usize, CassieError> {
    left.checked_add(right)
        .ok_or_else(|| CassieError::ResourceLimit("column batch output size overflow".to_string()))
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::time::Instant;

    use crate::config::CassieRuntimeLimits;
    use crate::runtime::QueryExecutionControls;

    use super::{project_column_batch_document, reserve_before_projecting, ColumnBatchRow};

    #[test]
    fn should_reject_projected_column_row_before_building_segment_output_given_low_memory() {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: 1,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let mut memory = controls.reserve_query_memory(0).expect("empty reservation");
        let projection_calls = Cell::new(0);
        let row = ColumnBatchRow {
            row_id: "row-0001".to_string(),
            values: BTreeMap::from([(
                "label".to_string(),
                serde_json::Value::String("projected-output".repeat(64)),
            )]),
        };

        // Act
        let retained_bytes = super::projected_document_retained_bytes(
            &row,
            &["id".to_string(), "label".to_string()],
        )
        .expect("retained size");
        let result = reserve_before_projecting(Some(&mut memory), retained_bytes, || {
            projection_calls.set(projection_calls.get() + 1);
            project_column_batch_document(None, row, &["label".to_string()])
        });

        // Assert
        assert!(matches!(
            result,
            Err(crate::app::CassieError::ResourceLimit(_))
        ));
        assert_eq!(projection_calls.get(), 0);
        assert_eq!(memory.bytes(), 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
