use super::schema_ops_helpers::{self, PendingFieldDrop};
use super::{CassieError, Midge, PendingFieldRename};

const UNIQUE_RESERVATION_RENAME_BATCH_SIZE: usize = 5_000;

pub(super) fn complete_field_rename_data(
    midge: &Midge,
    pending: &PendingFieldRename,
) -> Result<(), CassieError> {
    let collection = &pending.collection;
    let generation =
        begin_field_schema_change_maintenance(midge, collection, pending.target_generation)?;
    rename_unique_constraint_reservations(
        midge,
        collection,
        &pending.current_name,
        &pending.next_name,
    )?;
    schema_ops_helpers::rename_normalized_vector_records(
        midge,
        collection,
        &pending.current_name,
        &pending.next_name,
    );
    midge.rebuild_scalar_indexes_for_collection(collection)?;
    midge.rebuild_time_series_indexes_for_collection(collection)?;
    midge.complete_column_batch_maintenance(collection, generation, None)?;
    midge.complete_projection_hash_maintenance(collection, generation, 0)?;
    Ok(())
}

fn rename_unique_constraint_reservations(
    midge: &Midge,
    collection: &str,
    current_name: &str,
    next_name: &str,
) -> Result<(), CassieError> {
    let current_prefix =
        Midge::unique_constraint_reservation_field_prefix(collection, current_name);
    let next_prefix = Midge::unique_constraint_reservation_field_prefix(collection, next_name);
    loop {
        let entries = midge.raw_scan_prefix_page_for_collection(
            collection,
            &current_prefix,
            UNIQUE_RESERVATION_RENAME_BATCH_SIZE,
        )?;
        if entries.is_empty() {
            return Ok(());
        }
        let mut tx = midge.begin_data_rw_tx_for(collection)?;
        for (key, owner) in entries {
            let Some(value_suffix) = key.strip_prefix(current_prefix.as_slice()) else {
                continue;
            };
            let next_key = [next_prefix.as_slice(), value_suffix].concat();
            tx.put(next_key, owner, None).map_err(CassieError::from)?;
            tx.delete(key).map_err(CassieError::from)?;
        }
        tx.commit(midge.write_options_sync())
            .map_err(CassieError::from)?;
    }
}

pub(super) fn complete_field_drop_data(
    midge: &Midge,
    pending: &PendingFieldDrop,
) -> Result<(), CassieError> {
    let generation = begin_field_schema_change_maintenance(
        midge,
        &pending.collection,
        pending.target_generation,
    )?;
    schema_ops_helpers::delete_dropped_field_data(
        midge,
        &pending.collection,
        &pending.field,
        &schema_ops_helpers::DroppedCollectionIndexes {
            columns: pending.column_names.clone(),
            column_storage_ids: pending.column_storage_ids.clone(),
            scalars: pending.scalar_names.clone(),
            scalar_storage_ids: pending.scalar_storage_ids.clone(),
            time_series: pending.time_series_names.clone(),
            time_series_storage_ids: pending.time_series_storage_ids.clone(),
            fulltext: pending.fulltext_names.clone(),
            fulltext_storage_ids: pending.fulltext_storage_ids.clone(),
            vectors: pending.vector_names.clone(),
        },
    )?;
    midge.rebuild_scalar_indexes_for_collection(&pending.collection)?;
    midge.rebuild_time_series_indexes_for_collection(&pending.collection)?;
    midge.complete_column_batch_maintenance(&pending.collection, generation, None)?;
    midge.complete_projection_hash_maintenance(&pending.collection, generation, 0)?;
    Ok(())
}

fn begin_field_schema_change_maintenance(
    midge: &Midge,
    collection: &str,
    target_generation: u64,
) -> Result<u64, CassieError> {
    let current_generation = midge.collection_generation(collection)?;
    let generation = current_generation.max(target_generation);
    let mut tx = midge.begin_data_rw_tx_for(collection)?;
    if current_generation < target_generation {
        tx.put(
            Midge::collection_generation_key(collection),
            generation.to_be_bytes().to_vec(),
            None,
        )
        .map_err(CassieError::from)?;
    }
    Midge::record_column_batch_maintenance_debt_in_tx(&mut tx, collection, generation)?;
    Midge::record_projection_hash_maintenance_debt_in_tx(&mut tx, collection, generation)?;
    tx.commit(midge.write_options_sync())
        .map_err(CassieError::from)?;
    Ok(generation)
}
