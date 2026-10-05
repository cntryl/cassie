use std::collections::HashSet;
use std::mem::size_of;
use std::sync::Arc;

use crate::app::CassieError;
use crate::executor::retained_memory::{add, hash_table_bytes, mul};
use crate::runtime::QueryMemoryReservation;

use super::{
    batch, check_timeout, retention, Batch, BatchRow, JoinResult, QueryError, SourceExecution,
    SourceExecutionEnv, Value,
};

pub(super) fn finish_retained_join(
    env: &SourceExecutionEnv<'_>,
    joined: JoinResult,
) -> SourceExecution {
    check_timeout(env.controls)?;
    let (joined, diagnostic, switch_state) = joined.into_parts();
    if joined.is_empty() {
        drop(joined);
        check_timeout(env.controls)?;
        retention::publish(env, diagnostic, switch_state, 0);
        return Ok((Vec::new(), Vec::new()));
    }
    let (rows, mut memory) = joined.into_parts();
    let row_count = rows.len();
    let old_slots = mul(rows.capacity(), size_of::<BatchRow>())?;
    let batch_count = row_count.div_ceil(batch::DEFAULT_BATCH_SIZE);
    let new_backing = add(
        mul(row_count, size_of::<BatchRow>())?,
        mul(batch_count, size_of::<Batch>())?,
    )?;
    let lease_header = size_of::<QueryMemoryReservation>() + 2 * size_of::<usize>();
    memory.try_grow(add(new_backing, lease_header)?)?;
    let text_fields = controlled_text_fields(env, &rows, &mut memory)?;
    let mut batches = chunk_rows_exact(rows)?;
    // The previous joined Vec has now dropped. Keep the same reservation across the transfer,
    // releasing only its old row-slot backing after the replacement backing exists.
    memory.shrink_to(memory.bytes() - old_slots);
    let memory = Arc::new(memory);
    for row in batches.iter_mut().flatten() {
        let owned = std::mem::replace(row, BatchRow::from_projected_values(Vec::new()));
        *row = owned.with_query_memory(Some(Arc::clone(&memory)));
    }
    check_timeout(env.controls)?;
    retention::publish(env, diagnostic, switch_state, row_count);
    Ok((batches, text_fields))
}

fn controlled_text_fields(
    env: &SourceExecutionEnv<'_>,
    rows: &[BatchRow],
    memory: &mut QueryMemoryReservation,
) -> Result<Vec<String>, QueryError> {
    let longest_name = rows
        .iter()
        .flat_map(BatchRow::entries)
        .filter(|(_, value)| matches!(value, Value::String(_) | Value::Json(_)))
        .map(|(name, _)| name.len())
        .max();
    let Some(longest_name) = longest_name else {
        return Ok(Vec::new());
    };
    let _scratch = env
        .controls
        .reserve_query_memory(add(512, mul(longest_name, 32)?)?)?;
    let mut keys = HashSet::<String>::new();
    let mut fields = Vec::new();
    let mut table_bytes = 0;
    let mut key_heap = 0;
    for row in rows {
        check_timeout(env.controls)?;
        for (name, value) in row.entries() {
            if !matches!(value, Value::String(_) | Value::Json(_)) {
                continue;
            }
            let key = crate::sql::ColumnIdentifierPath::stored_row_lookup_key(name);
            if keys.contains(&key) {
                continue;
            }
            let next_table = hash_table_bytes::<String>(add(keys.len(), 1)?)?;
            let field_bytes = add(size_of::<String>(), name.len())?;
            // Rehashing can retain the old table while allocating its replacement. Its final
            // table charge already lives in `memory`; admit the old/new overlap separately.
            let _table_overlap = (keys.len() == keys.capacity())
                .then(|| env.controls.reserve_query_memory(table_bytes))
                .transpose()?;
            memory.try_grow(add(
                field_bytes,
                add(key.capacity(), next_table - table_bytes)?,
            )?)?;
            keys.try_reserve(1)
                .map_err(|error| allocation_error(&error))?;
            fields
                .try_reserve_exact(1)
                .map_err(|error| allocation_error(&error))?;
            table_bytes = next_table;
            key_heap = add(key_heap, key.capacity())?;
            keys.insert(key);
            fields.push(name.clone());
        }
    }
    drop(keys);
    memory.shrink_to(memory.bytes() - add(table_bytes, key_heap)?);
    Ok(fields)
}

fn chunk_rows_exact(rows: Vec<BatchRow>) -> Result<Vec<Batch>, QueryError> {
    let mut rows = rows.into_iter();
    let mut batches = Vec::new();
    batches
        .try_reserve_exact(rows.len().div_ceil(batch::DEFAULT_BATCH_SIZE))
        .map_err(|error| allocation_error(&error))?;
    while !rows.as_slice().is_empty() {
        let count = rows.len().min(batch::DEFAULT_BATCH_SIZE);
        let mut batch = Vec::new();
        batch
            .try_reserve_exact(count)
            .map_err(|error| allocation_error(&error))?;
        batch.extend(rows.by_ref().take(count));
        batches.push(batch);
    }
    Ok(batches)
}

fn allocation_error(error: &std::collections::TryReserveError) -> QueryError {
    CassieError::ResourceLimit(format!("unable to retain joined output: {error}")).into()
}
