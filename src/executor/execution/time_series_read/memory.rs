use crate::app::CassieError;
use crate::catalog::CollectionSchema;
use crate::executor::batch::{Batch, BatchRow, DEFAULT_BATCH_SIZE};
use crate::midge::adapter::DocumentRef;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(super) struct AccountedTimeSeriesBatches {
    pub(super) batches: Vec<Batch>,
    pub(super) memory: QueryMemoryReservation,
}

pub(super) fn finalize_batches(
    batches: Vec<Batch>,
    controls: &QueryExecutionControls,
) -> Result<(Vec<BatchRow>, QueryMemoryReservation), crate::executor::QueryError> {
    super::check_timeout(controls)?;
    let memory = super::ensure_query_memory_budget(controls, &batches)?;
    let rows = crate::executor::batch::try_flatten_batches(batches)?;
    Ok((rows, memory))
}

pub(super) fn document_batches(
    documents: Vec<DocumentRef>,
    fields: &[String],
    schema: Option<&CollectionSchema>,
    controls: &QueryExecutionControls,
) -> Result<AccountedTimeSeriesBatches, crate::executor::QueryError> {
    let retained_bytes = time_series_batch_bytes(&documents, fields)?;
    let (batches, memory) = reserve_before_building(controls, retained_bytes, || {
        build_document_batches(documents, fields, schema)
    })?;
    Ok(AccountedTimeSeriesBatches { batches, memory })
}

fn reserve_before_building<T>(
    controls: &QueryExecutionControls,
    retained_bytes: usize,
    build: impl FnOnce() -> Result<T, CassieError>,
) -> Result<(T, QueryMemoryReservation), CassieError> {
    let memory = controls.reserve_query_memory(retained_bytes)?;
    let value = build()?;
    Ok((value, memory))
}

fn build_document_batches(
    documents: Vec<DocumentRef>,
    fields: &[String],
    schema: Option<&CollectionSchema>,
) -> Result<Vec<Batch>, CassieError> {
    let batch_count = documents.len().div_ceil(DEFAULT_BATCH_SIZE);
    let mut batches = Vec::new();
    try_reserve(&mut batches, batch_count)?;
    let mut remaining = documents.len();
    let mut current = Vec::new();
    try_reserve(&mut current, remaining.min(DEFAULT_BATCH_SIZE))?;
    for document in documents {
        current.push(super::document_to_row(document, fields, schema));
        remaining = remaining.saturating_sub(1);
        if current.len() == DEFAULT_BATCH_SIZE {
            batches.push(std::mem::take(&mut current));
            try_reserve(&mut current, remaining.min(DEFAULT_BATCH_SIZE))?;
        }
    }
    if !current.is_empty() {
        batches.push(current);
    }
    Ok(batches)
}

fn try_reserve<T>(values: &mut Vec<T>, additional: usize) -> Result<(), CassieError> {
    values.try_reserve_exact(additional).map_err(|error| {
        CassieError::ResourceLimit(format!(
            "unable to retain controlled time-series output: {error}"
        ))
    })
}

fn time_series_batch_bytes(
    documents: &[DocumentRef],
    fields: &[String],
) -> Result<usize, CassieError> {
    let container_bytes = documents
        .len()
        .div_ceil(DEFAULT_BATCH_SIZE)
        .checked_mul(std::mem::size_of::<Batch>())
        .ok_or_else(time_series_output_overflow)?;
    documents
        .iter()
        .try_fold(container_bytes, |total, document| {
            total
                .checked_add(time_series_row_bytes(document, fields)?)
                .ok_or_else(time_series_output_overflow)
        })
}

fn time_series_row_bytes(document: &DocumentRef, fields: &[String]) -> Result<usize, CassieError> {
    let entry_count = fields
        .len()
        .checked_add(1)
        .ok_or_else(time_series_output_overflow)?;
    let inline_bytes = entry_count
        .checked_mul(
            std::mem::size_of::<(String, crate::types::Value)>()
                .saturating_add(4 * std::mem::size_of::<usize>()),
        )
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<BatchRow>()))
        .ok_or_else(time_series_output_overflow)?;
    let id_bytes = document
        .id
        .len()
        .checked_add(2)
        .ok_or_else(time_series_output_overflow)?;
    fields.iter().try_fold(
        inline_bytes
            .checked_add(id_bytes)
            .ok_or_else(time_series_output_overflow)?,
        |total, field| {
            let value_bytes =
                super::payload_field(&document.payload, field).map_or(0, json_retained_bytes);
            total
                .checked_add(field.len().saturating_mul(2))
                .and_then(|bytes| bytes.checked_add(value_bytes))
                .ok_or_else(time_series_output_overflow)
        },
    )
}

fn json_retained_bytes(value: &serde_json::Value) -> usize {
    let inline = std::mem::size_of::<serde_json::Value>();
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            inline
        }
        serde_json::Value::String(value) => inline.saturating_add(value.len()),
        serde_json::Value::Array(values) => values.iter().fold(inline, |bytes, value| {
            bytes.saturating_add(json_retained_bytes(value))
        }),
        serde_json::Value::Object(values) => values.iter().fold(inline, |bytes, (key, value)| {
            bytes
                .saturating_add(std::mem::size_of::<String>())
                .saturating_add(key.len())
                .saturating_add(json_retained_bytes(value))
        }),
    }
}

fn time_series_output_overflow() -> CassieError {
    CassieError::ResourceLimit("time-series output size overflow".to_string())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::Instant;

    use crate::config::CassieRuntimeLimits;
    use crate::executor::batch::BatchRow;
    use crate::runtime::{QueryCancellationHandle, QueryExecutionControls};
    use crate::types::Value;

    use super::{finalize_batches, reserve_before_building};

    #[test]
    fn should_reject_time_series_output_before_building_retained_rows() {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: 3,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let build_calls = Cell::new(0);

        // Act
        let result = reserve_before_building(&controls, 4, || {
            build_calls.set(build_calls.get() + 1);
            Ok(())
        });

        // Assert
        assert!(matches!(
            result,
            Err(crate::app::CassieError::ResourceLimit(_))
        ));
        assert_eq!(build_calls.get(), 0);
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_reject_late_time_series_output_before_flattening_given_low_memory() {
        // Arrange
        let controls = QueryExecutionControls::from_limits(
            &CassieRuntimeLimits {
                query_memory_budget_bytes: 1,
                ..CassieRuntimeLimits::default()
            },
            Instant::now(),
        );
        let batches = vec![vec![BatchRow::new(vec![(
            "label".to_string(),
            Value::String("late-output".repeat(64)),
        )])]];

        // Act
        let result = finalize_batches(batches, &controls);

        // Assert
        assert!(matches!(
            result,
            Err(crate::executor::QueryError::Cassie(
                crate::app::CassieError::ResourceLimit(_)
            ))
        ));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }

    #[test]
    fn should_cancel_late_time_series_output_before_flattening_rows() {
        // Arrange
        let cancellation = QueryCancellationHandle::new();
        let controls = QueryExecutionControls::with_cancellation(
            &CassieRuntimeLimits::default(),
            Instant::now(),
            cancellation.clone(),
        );
        let batches = vec![vec![BatchRow::new(vec![(
            "label".to_string(),
            Value::String("late-output".to_string()),
        )])]];
        cancellation.cancel();

        // Act
        let result = finalize_batches(batches, &controls);

        // Assert
        assert!(matches!(
            result,
            Err(crate::executor::QueryError::General(message)) if message == "query canceled"
        ));
        assert_eq!(controls.current_query_memory_bytes(), 0);
    }
}
