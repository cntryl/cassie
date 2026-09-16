use super::{CassieError, Midge, RangeHashRecord, RootHashRecord, RowHashRecord};

const PROJECTION_OUTPUT_FLUSH_INTERVAL_BATCHES: usize = 1;

pub(crate) fn write_ranges(
    item_count: usize,
    batch_size: usize,
) -> impl Iterator<Item = std::ops::Range<usize>> {
    (0..item_count)
        .step_by(batch_size)
        .map(move |start| start..start.saturating_add(batch_size).min(item_count))
}

pub(crate) fn should_flush_projection_output_batch(
    completed_batches: usize,
    batch_count: usize,
) -> bool {
    completed_batches == batch_count
        || completed_batches.is_multiple_of(PROJECTION_OUTPUT_FLUSH_INTERVAL_BATCHES)
}

pub(super) fn write_row_hash_record_to_tx(
    tx: &mut cntryl_midge::Transaction,
    record: &RowHashRecord,
) -> Result<(), CassieError> {
    tx.put(
        Midge::row_hash_key(&record.collection, &record.row_id),
        serde_json::to_vec(record).map_err(|error| CassieError::Parse(error.to_string()))?,
        None,
    )
    .map_err(CassieError::from)?;
    Ok(())
}

pub(super) fn write_range_hash_record_to_tx(
    tx: &mut cntryl_midge::Transaction,
    record: &RangeHashRecord,
) -> Result<(), CassieError> {
    tx.put(
        Midge::range_hash_key(&record.collection, record.range_id),
        serde_json::to_vec(record).map_err(|error| CassieError::Parse(error.to_string()))?,
        None,
    )
    .map_err(CassieError::from)?;
    Ok(())
}

pub(super) fn write_root_hash_record_to_tx(
    tx: &mut cntryl_midge::Transaction,
    record: &RootHashRecord,
) -> Result<(), CassieError> {
    tx.put(
        Midge::root_hash_key(&record.collection),
        serde_json::to_vec(record).map_err(|error| CassieError::Parse(error.to_string()))?,
        None,
    )
    .map_err(CassieError::from)?;
    Ok(())
}

pub(super) fn delete_keys_from_tx(
    tx: &mut cntryl_midge::Transaction,
    keys: &[Vec<u8>],
) -> Result<(), CassieError> {
    for key in keys {
        tx.delete(key.clone()).map_err(CassieError::from)?;
    }
    Ok(())
}
