//! Pre-admitted result row for a JSON object-key expansion.
use super::{observe, BatchRow, Progress};
use crate::app::CassieError;
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};
use crate::types::Value;

pub(super) struct PreparedRow {
    pub(super) row: BatchRow,
    pub(super) memory: QueryMemoryReservation,
}

pub(super) fn prepare(
    name: &str,
    key: &str,
    source: &BatchRow,
    controls: &QueryExecutionControls,
) -> Result<PreparedRow, CassieError> {
    let bytes = name
        .len()
        .checked_add(key.len())
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<(String, Value)>()))
        .ok_or_else(|| CassieError::ResourceLimit("JSON object-key row size overflow".into()))?;
    let memory = controls.reserve_query_memory(bytes)?;
    let name = copy(name, Progress::Name, controls)?;
    let key = copy(key, Progress::Key, controls)?;
    let mut entries = Vec::new();
    entries
        .try_reserve_exact(1)
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if entries.capacity() > 1 {
        return Err(CassieError::ResourceLimit(
            "JSON object-key row exceeded its admission".into(),
        ));
    }
    entries.push((name, Value::String(key)));
    observe(Progress::Entries);
    controls.check_interruption()?;
    let row = BatchRow::from_projected_values(entries)
        .with_optional_data_types(source.shared_data_types());
    Ok(PreparedRow { row, memory })
}

fn copy(
    input: &str,
    stage: Progress,
    controls: &QueryExecutionControls,
) -> Result<String, CassieError> {
    let mut output = String::new();
    output
        .try_reserve_exact(input.len())
        .map_err(|error| crate::runtime::owned_memory::allocation_error(&error))?;
    if output.capacity() > input.len() {
        return Err(CassieError::ResourceLimit(
            "JSON object-key text exceeded its admission".into(),
        ));
    }
    let mut start = 0;
    while start < input.len() {
        controls.check_interruption()?;
        let mut end = start.saturating_add(1024).min(input.len());
        while !input.is_char_boundary(end) {
            end -= 1;
        }
        output.push_str(&input[start..end]);
        start = end;
        observe(stage);
        controls.check_interruption()?;
    }
    Ok(output)
}
