use super::{
    collect_scan, CassieError, FieldConstraint, IndexKind, IndexMeta, Midge, ProjectionMeta, Query,
    RetentionPolicyMeta,
};
use crate::catalog::{canonical_relation_name, local_name, ColumnBatchMetadata, RelationId};

pub(super) fn clear_pending_collection_rename(
    midge: &Midge,
    current: &str,
    next: &str,
) -> Result<(), CassieError> {
    let mut tx = midge.begin_schema_rw_tx()?;
    tx.delete(Midge::schema_operation_key(current, next))
        .map_err(CassieError::from)?;
    tx.commit(cntryl_midge::WriteOptions::sync())
        .map_err(CassieError::from)
}

pub(super) fn clear_pending_field_drop(
    midge: &Midge,
    collection: &str,
    field: &str,
) -> Result<(), CassieError> {
    let mut tx = midge.begin_schema_rw_tx()?;
    tx.delete(Midge::field_drop_operation_key(collection, field))
        .map_err(CassieError::from)?;
    tx.commit(cntryl_midge::WriteOptions::sync())
        .map_err(CassieError::from)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct PendingFieldDrop {
    pub(super) collection: String,
    pub(super) field: String,
    #[serde(default)]
    pub(super) column_names: Vec<String>,
    #[serde(default)]
    pub(super) column_storage_ids: Vec<(u64, u64)>,
    pub(super) scalar_names: Vec<String>,
    #[serde(default)]
    pub(super) scalar_storage_ids: Vec<(u64, u64)>,
    pub(super) time_series_names: Vec<String>,
    #[serde(default)]
    pub(super) time_series_storage_ids: Vec<(u64, u64)>,
    #[serde(default)]
    pub(super) fulltext_names: Vec<String>,
    #[serde(default)]
    pub(super) fulltext_storage_ids: Vec<(u64, u64)>,
    #[serde(default)]
    pub(super) vector_names: Vec<String>,
}

pub(super) struct DroppedCollectionIndexes {
    pub columns: Vec<String>,
    pub column_storage_ids: Vec<(u64, u64)>,
    pub scalars: Vec<String>,
    pub scalar_storage_ids: Vec<(u64, u64)>,
    pub time_series: Vec<String>,
    pub time_series_storage_ids: Vec<(u64, u64)>,
    pub fulltext: Vec<String>,
    pub fulltext_storage_ids: Vec<(u64, u64)>,
    pub vectors: Vec<String>,
}

pub(super) fn drop_referencing_indexes_in_tx(
    tx: &mut cntryl_midge::Transaction,
    collection: &str,
    field: &str,
) -> Result<DroppedCollectionIndexes, CassieError> {
    let index_prefix = Midge::index_collection_prefix(collection);
    let indexes = collect_scan(
        tx.scan(&Query::new().prefix(index_prefix.into()))
            .map_err(CassieError::from)?,
    )?;
    let mut dropped_column_index_keys = Vec::new();
    let mut dropped_column_indexes = Vec::new();
    let mut dropped_scalar_indexes = Vec::new();
    let mut dropped_time_series_indexes = Vec::new();
    let mut dropped_fulltext_indexes = Vec::new();
    let mut dropped_vector_indexes = Vec::new();

    for (key, value) in indexes {
        let Ok(metadata) = serde_json::from_slice::<IndexMeta>(&value) else {
            continue;
        };
        let partition_references_field =
            metadata.options.get("partition_by").is_some_and(|fields| {
                fields
                    .split(',')
                    .map(str::trim)
                    .any(|candidate| candidate.eq_ignore_ascii_case(field))
            });
        let references_field = partition_references_field
            || metadata
                .normalized_fields()
                .iter()
                .chain(metadata.normalized_include_fields().iter())
                .any(|candidate| candidate.eq_ignore_ascii_case(field));
        if !references_field {
            continue;
        }
        match metadata.kind {
            IndexKind::Column => {
                dropped_column_index_keys.push(key);
                dropped_column_indexes.push(metadata);
            }
            IndexKind::Scalar => dropped_scalar_indexes.push((key, metadata)),
            IndexKind::TimeSeries => dropped_time_series_indexes.push((key, metadata)),
            IndexKind::FullText => dropped_fulltext_indexes.push((key, metadata)),
            IndexKind::Vector => {
                tx.delete(key).map_err(CassieError::from)?;
                tx.delete(Midge::vector_index_key(collection, &metadata.field))
                    .map_err(CassieError::from)?;
                dropped_vector_indexes.push(metadata.field);
            }
            IndexKind::Hybrid => {}
        }
    }

    for key in dropped_column_index_keys {
        tx.delete(key).map_err(CassieError::from)?;
    }
    let column_storage_ids = dropped_column_indexes
        .iter()
        .filter_map(|index| Some((index.relation_id()?, index.storage_id()?)))
        .collect();
    let column_names = dropped_column_indexes
        .into_iter()
        .map(|index| index.name)
        .collect();

    let mut scalar_names = Vec::new();
    let mut scalar_storage_ids = Vec::new();
    for (key, index) in dropped_scalar_indexes {
        tx.delete(key).map_err(CassieError::from)?;
        if let (Some(relation_id), Some(index_id)) = (index.relation_id(), index.storage_id()) {
            scalar_storage_ids.push((relation_id, index_id));
        }
        scalar_names.push(index.name);
    }

    let mut time_series_names = Vec::new();
    let mut time_series_storage_ids = Vec::new();
    for (key, index) in dropped_time_series_indexes {
        tx.delete(key).map_err(CassieError::from)?;
        if let (Some(relation_id), Some(index_id)) = (index.relation_id(), index.storage_id()) {
            time_series_storage_ids.push((relation_id, index_id));
        }
        time_series_names.push(index.name);
    }

    let mut fulltext_names = Vec::new();
    let mut fulltext_storage_ids = Vec::new();
    for (key, index) in dropped_fulltext_indexes {
        tx.delete(key).map_err(CassieError::from)?;
        if let (Some(relation_id), Some(index_id)) = (index.relation_id(), index.storage_id()) {
            fulltext_storage_ids.push((relation_id, index_id));
        }
        fulltext_names.push(index.name);
    }

    Ok(DroppedCollectionIndexes {
        columns: column_names,
        column_storage_ids,
        scalars: scalar_names,
        scalar_storage_ids,
        time_series: time_series_names,
        time_series_storage_ids,
        fulltext: fulltext_names,
        fulltext_storage_ids,
        vectors: dropped_vector_indexes,
    })
}

pub(super) fn delete_dropped_field_data(
    midge: &Midge,
    collection: &str,
    field: &str,
    dropped_indexes: &DroppedCollectionIndexes,
) -> Result<(), CassieError> {
    let (relation_id, field_id) = midge.vector_storage_ids(collection, field)?;
    let mut data_tx = midge.begin_data_rw_tx_for(collection)?;
    Midge::delete_normalized_vector_keys_with_prefix(
        &mut data_tx,
        Midge::normalized_vector_prefix(relation_id, field_id),
    )?;
    for vector_field in &dropped_indexes.vectors {
        let (relation_id, field_id) = midge.vector_storage_ids(collection, vector_field)?;
        Midge::delete_normalized_vector_keys_with_prefix(
            &mut data_tx,
            Midge::normalized_vector_prefix(relation_id, field_id),
        )?;
        data_tx
            .delete(Midge::vector_index_state_key(relation_id, field_id))
            .map_err(CassieError::from)?;
    }
    for (relation_id, index_id) in &dropped_indexes.scalar_storage_ids {
        Midge::delete_keys_with_prefix(
            &mut data_tx,
            Midge::scalar_index_data_prefix(*relation_id, *index_id),
        )?;
    }
    for (relation_id, index_id) in &dropped_indexes.time_series_storage_ids {
        Midge::delete_keys_with_prefix(
            &mut data_tx,
            Midge::time_series_index_data_prefix(*relation_id, *index_id),
        )?;
    }
    for (relation_id, index_id) in &dropped_indexes.column_storage_ids {
        Midge::delete_keys_with_prefix(
            &mut data_tx,
            Midge::column_batch_index_prefix(*relation_id, *index_id),
        )?;
    }
    for (relation_id, index_id) in &dropped_indexes.fulltext_storage_ids {
        Midge::delete_keys_with_prefix(
            &mut data_tx,
            Midge::fulltext_index_artifact_prefix(*relation_id, *index_id),
        )?;
    }
    for index_name in &dropped_indexes.scalars {
        Midge::delete_keys_with_prefix(
            &mut data_tx,
            Midge::unique_scalar_index_reservation_prefix(collection, index_name),
        )?;
    }
    Midge::delete_keys_with_prefix(
        &mut data_tx,
        Midge::unique_constraint_reservation_field_prefix(collection, field),
    )?;
    data_tx
        .commit(super::WriteOptions::sync())
        .map_err(CassieError::from)?;
    Ok(())
}

pub(super) fn rename_constraints_in_tx(
    tx: &mut cntryl_midge::Transaction,
    collection: &str,
    current_name: &str,
    next_name: &str,
) -> Result<(), CassieError> {
    let Some(raw_constraints) = tx
        .get(&Midge::constraints_key(collection))
        .map_err(CassieError::from)?
    else {
        return Ok(());
    };
    let mut constraints: Vec<FieldConstraint> =
        serde_json::from_slice(&raw_constraints).map_err(|error| {
            CassieError::Parse(format!(
                "invalid constraint metadata for '{collection}': {error}"
            ))
        })?;
    let mut changed = false;
    for constraint in &mut constraints {
        if constraint.field.eq_ignore_ascii_case(current_name) {
            constraint.field = next_name.to_string();
            changed = true;
        }
        if let Some(check) = constraint.check.as_mut() {
            if check.field.eq_ignore_ascii_case(current_name) {
                check.field = next_name.to_string();
                changed = true;
            }
        }
    }
    if changed {
        let value = serde_json::to_vec(&constraints)
            .map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Midge::constraints_key(collection), value, None)
            .map_err(CassieError::from)?;
    }
    Ok(())
}

pub(super) fn rename_indexes_in_tx(
    tx: &mut cntryl_midge::Transaction,
    collection: &str,
    current_name: &str,
    next_name: &str,
) -> Result<(), CassieError> {
    let index_prefix = Midge::index_collection_prefix(collection);
    let indexes = collect_scan(
        tx.scan(&Query::new().prefix(index_prefix.into()))
            .map_err(CassieError::from)?,
    )?;
    let mut index_keys = Vec::new();
    for (key, _value) in indexes {
        index_keys.push(key);
    }
    for key in index_keys {
        let Some(raw_value) = tx.get(&key).map_err(CassieError::from)? else {
            continue;
        };
        let Ok(mut metadata) = serde_json::from_slice::<IndexMeta>(&raw_value) else {
            continue;
        };
        let mut changed = metadata.rename_field(current_name, next_name);
        if metadata.kind == IndexKind::TimeSeries {
            if let Some(raw_partition_by) = metadata.options.get("partition_by").cloned() {
                let partition_by = raw_partition_by
                    .split(',')
                    .map(str::trim)
                    .filter(|field| !field.is_empty())
                    .map(|field| {
                        if field.eq_ignore_ascii_case(current_name) {
                            next_name.to_string()
                        } else {
                            field.to_string()
                        }
                    })
                    .collect::<Vec<_>>();
                let next_partition_by = partition_by.join(",");
                if next_partition_by != raw_partition_by {
                    metadata
                        .options
                        .insert("partition_by".to_string(), next_partition_by);
                    changed = true;
                }
            }
        }
        if changed {
            let value = serde_json::to_vec(&metadata)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            tx.put(key, value, None).map_err(CassieError::from)?;
        }
    }
    Ok(())
}

pub(super) fn rename_vector_indexes_in_tx(
    tx: &mut cntryl_midge::Transaction,
    collection: &str,
    current_name: &str,
    next_name: &str,
) -> Result<(), CassieError> {
    let vector_prefix = Midge::vector_index_collection_prefix(collection);
    let vector_indexes = collect_scan(
        tx.scan(&Query::new().prefix(vector_prefix.into()))
            .map_err(CassieError::from)?,
    )?;
    let mut vector_keys = Vec::new();
    for (key, _value) in vector_indexes {
        vector_keys.push(key);
    }

    for key in vector_keys {
        let Some(raw_value) = tx.get(&key).map_err(CassieError::from)? else {
            continue;
        };
        let Ok(mut record) =
            serde_json::from_slice::<crate::embeddings::VectorIndexRecord>(&raw_value)
        else {
            continue;
        };

        let mut changed = false;
        let mut next_key = key.clone();
        if record.field.eq_ignore_ascii_case(current_name) {
            record.field = next_name.to_string();
            next_key = Midge::vector_index_key(&record.collection, &record.field);
            changed = true;
        }
        if record.source_field.eq_ignore_ascii_case(current_name) {
            record.source_field = next_name.to_string();
            changed = true;
        }

        if changed {
            if next_key != key {
                tx.delete(key).map_err(CassieError::from)?;
            }
            let value = serde_json::to_vec(&record)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            tx.put(next_key, value, None).map_err(CassieError::from)?;
        }
    }

    Ok(())
}

pub(super) fn rename_normalized_vector_records(
    _midge: &Midge,
    _collection: &str,
    _current_name: &str,
    _next_name: &str,
) {
}

pub(super) fn rename_collection_projection_metadata(
    tx: &mut cntryl_midge::Transaction,
    current_name: &str,
    next_name: &str,
) -> Result<(), CassieError> {
    let current_projection_key = Midge::projection_key(current_name);
    let Some(projection_bytes) = tx.get(&current_projection_key).map_err(CassieError::from)? else {
        return Ok(());
    };
    let mut metadata: ProjectionMeta =
        serde_json::from_slice(&projection_bytes).map_err(|error| {
            CassieError::Parse(format!(
                "invalid projection metadata for '{current_name}': {error}"
            ))
        })?;
    if metadata.projection_id == current_name {
        metadata.projection_id = next_name.to_string();
    }
    metadata.collection = next_name.to_string();
    tx.delete(current_projection_key)
        .map_err(CassieError::from)?;
    Midge::save_projection_metadata_to_tx(tx, &metadata)
}

pub(super) fn rename_collection_schema_entries(
    tx: &mut cntryl_midge::Transaction,
    current_name: &str,
    next_name: &str,
) -> Result<(), CassieError> {
    let current_schema_key = Midge::collection_schema_key(current_name);
    let current_schema_bytes = tx
        .get(&current_schema_key)
        .map_err(CassieError::from)?
        .ok_or_else(|| CassieError::CollectionNotFound(current_name.to_string()))?;

    let next_schema_key = Midge::collection_schema_key(next_name);
    if tx
        .get(&next_schema_key)
        .map_err(CassieError::from)?
        .is_some()
    {
        return Err(CassieError::Unsupported(format!(
            "collection '{next_name}' already exists"
        )));
    }

    tx.delete(current_schema_key).map_err(CassieError::from)?;
    tx.put(next_schema_key, current_schema_bytes.to_vec(), None)
        .map_err(CassieError::from)?;

    let current_row_schema_key = Midge::row_schema_key(current_name);
    if let Some(row_schema_bytes) = tx.get(&current_row_schema_key).map_err(CassieError::from)? {
        tx.delete(current_row_schema_key)
            .map_err(CassieError::from)?;
        tx.put(
            Midge::row_schema_key(next_name),
            row_schema_bytes.to_vec(),
            None,
        )
        .map_err(CassieError::from)?;
    }
    Midge::rename_collection_metadata_to_tx(tx, current_name, next_name)?;
    rename_collection_projection_metadata(tx, current_name, next_name)?;

    let mut collections = Midge::load_collections(tx)?;
    if let Some(position) = collections.iter().position(|entry| entry == current_name) {
        collections[position] = next_name.to_string();
        collections.sort();
        collections.dedup();
        Midge::save_collections(tx, &collections)?;
    }
    Ok(())
}

pub(super) fn transfer_collection_sidecars(
    tx: &mut cntryl_midge::Transaction,
    current_name: &str,
    next_name: &str,
) -> Result<(), CassieError> {
    let current_constraints_key = Midge::constraints_key(current_name);
    if let Some(raw) = tx
        .get(&current_constraints_key)
        .map_err(CassieError::from)?
    {
        tx.delete(current_constraints_key)
            .map_err(CassieError::from)?;
        tx.put(Midge::constraints_key(next_name), raw.to_vec(), None)
            .map_err(CassieError::from)?;
    }

    let current_cardinality_key = Midge::cardinality_key(current_name);
    if let Some(raw) = tx
        .get(current_cardinality_key.as_slice())
        .map_err(CassieError::from)?
    {
        tx.delete(current_cardinality_key)
            .map_err(CassieError::from)?;
        tx.put(Midge::cardinality_key(next_name), raw.to_vec(), None)
            .map_err(CassieError::from)?;
    }
    Ok(())
}

pub(super) fn rename_collection_vector_indexes(
    tx: &mut cntryl_midge::Transaction,
    current_name: &str,
    next_name: &str,
) -> Result<(), CassieError> {
    let vector_prefix = Midge::vector_index_collection_prefix(current_name);
    let vector_indexes = collect_scan(
        tx.scan(&Query::new().prefix(vector_prefix.into()))
            .map_err(CassieError::from)?,
    )?;
    let mut vector_keys = Vec::new();
    for (key, _value) in vector_indexes {
        vector_keys.push(key);
    }

    for key in vector_keys {
        let Some(raw_value) = tx.get(&key).map_err(CassieError::from)? else {
            continue;
        };
        let Ok(mut record) =
            serde_json::from_slice::<crate::embeddings::VectorIndexRecord>(&raw_value)
        else {
            continue;
        };
        record.collection = next_name.to_string();
        tx.delete(key).map_err(CassieError::from)?;
        let next_key = Midge::vector_index_key(&record.collection, &record.field);
        let value =
            serde_json::to_vec(&record).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(next_key, value, None).map_err(CassieError::from)?;
    }
    Ok(())
}

pub(super) fn rename_collection_indexes(
    tx: &mut cntryl_midge::Transaction,
    current_name: &str,
    next_name: &str,
) -> Result<(), CassieError> {
    let index_prefix = Midge::index_collection_prefix(current_name);
    let indexes = collect_scan(
        tx.scan(&Query::new().prefix(index_prefix.into()))
            .map_err(CassieError::from)?,
    )?;
    let mut index_keys = Vec::new();
    for (key, _value) in indexes {
        index_keys.push(key);
    }
    for key in index_keys {
        let Some(raw_value) = tx.get(&key).map_err(CassieError::from)? else {
            continue;
        };
        let Ok(mut metadata) = serde_json::from_slice::<IndexMeta>(&raw_value) else {
            continue;
        };
        metadata.name = renamed_scoped_relation_name(current_name, next_name, &metadata.name);
        metadata.collection = next_name.to_string();
        tx.delete(key).map_err(CassieError::from)?;
        let next_key = Midge::index_key(&metadata.collection, &metadata.name);
        let value =
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(next_key, value, None).map_err(CassieError::from)?;
    }
    Ok(())
}

pub(super) fn rename_collection_retention_policies(
    tx: &mut cntryl_midge::Transaction,
    current_name: &str,
    next_name: &str,
) -> Result<(), CassieError> {
    let retention_scan = collect_scan(
        tx.scan(&Query::new().prefix(Midge::retention_prefix().into()))
            .map_err(CassieError::from)?,
    )?;
    let mut retention_entries = Vec::new();
    for (key, value) in retention_scan {
        let Ok(mut policy) = serde_json::from_slice::<RetentionPolicyMeta>(&value) else {
            continue;
        };
        if policy.collection == current_name {
            policy.collection = next_name.to_string();
            retention_entries.push((key, policy));
        }
    }
    for (key, policy) in retention_entries {
        tx.delete(key).map_err(CassieError::from)?;
        let value =
            serde_json::to_vec(&policy).map_err(|error| CassieError::Parse(error.to_string()))?;
        tx.put(Midge::retention_key(&policy.name), value, None)
            .map_err(CassieError::from)?;
    }
    Ok(())
}

pub(super) fn rename_collection_column_batch_metadata(
    tx: &mut cntryl_midge::Transaction,
    current_name: &str,
    next_name: &str,
    relation_id: u64,
) -> Result<(), CassieError> {
    let entries = collect_scan(
        tx.scan(&Query::new().prefix(Midge::column_batch_collection_prefix(relation_id).into()))
            .map_err(CassieError::from)?,
    )?;
    for (key, value) in entries {
        let Ok(mut metadata) = serde_json::from_slice::<ColumnBatchMetadata>(&value) else {
            continue;
        };
        let next_index_name =
            renamed_scoped_relation_name(current_name, next_name, &metadata.index_name);
        metadata.collection = next_name.to_string();
        metadata.index_name.clone_from(&next_index_name);
        tx.put(
            key,
            serde_json::to_vec(&metadata).map_err(|error| CassieError::Parse(error.to_string()))?,
            None,
        )
        .map_err(CassieError::from)?;
    }
    Ok(())
}

fn renamed_scoped_relation_name(
    current_name: &str,
    next_name: &str,
    relation_name: &str,
) -> String {
    let Some(current_relation) = RelationId::parse_canonical(current_name) else {
        return relation_name.to_string();
    };
    let Some(next_relation) = RelationId::parse_canonical(next_name) else {
        return relation_name.to_string();
    };
    let Some(relation) = RelationId::parse_canonical(relation_name) else {
        return relation_name.to_string();
    };
    if relation
        .database
        .eq_ignore_ascii_case(&current_relation.database)
        && relation
            .schema
            .eq_ignore_ascii_case(&current_relation.schema)
    {
        return canonical_relation_name(
            &next_relation.database,
            &next_relation.schema,
            &local_name(relation_name),
        );
    }
    relation_name.to_string()
}

type CollectionRenamePrefix = (Vec<u8>, Vec<u8>, bool);

fn collection_rename_prefixes(
    current_name: &str,
    next_name: &str,
    relation_id: u64,
) -> [CollectionRenamePrefix; 13] {
    [
        (
            Midge::row_prefix(relation_id),
            Midge::row_prefix(relation_id),
            false,
        ),
        (
            Midge::doc_prefix(current_name),
            Midge::doc_prefix(next_name),
            false,
        ),
        (
            Midge::scalar_index_collection_prefix(relation_id),
            Midge::scalar_index_collection_prefix(relation_id),
            false,
        ),
        (
            Midge::time_series_index_collection_prefix(relation_id),
            Midge::time_series_index_collection_prefix(relation_id),
            false,
        ),
        (
            Midge::fulltext_index_collection_prefix(relation_id),
            Midge::fulltext_index_collection_prefix(relation_id),
            false,
        ),
        (
            Midge::normalized_vector_collection_prefix(relation_id),
            Midge::normalized_vector_collection_prefix(relation_id),
            false,
        ),
        (
            Midge::vector_index_state_prefix(relation_id),
            Midge::vector_index_state_prefix(relation_id),
            false,
        ),
        (
            super::key_encoding::unique_constraint_reservation_prefix(current_name),
            super::key_encoding::unique_constraint_reservation_prefix(next_name),
            false,
        ),
        (
            super::key_encoding::unique_index_reservation_prefix(current_name),
            super::key_encoding::unique_index_reservation_prefix(next_name),
            false,
        ),
        (
            Midge::column_store_collection_prefix(relation_id),
            Midge::column_store_collection_prefix(relation_id),
            false,
        ),
        (
            Midge::column_batch_collection_prefix(relation_id),
            Midge::column_batch_collection_prefix(relation_id),
            false,
        ),
        (
            Midge::row_hash_prefix(current_name),
            Midge::row_hash_prefix(next_name),
            false,
        ),
        (
            Midge::range_hash_prefix(current_name),
            Midge::range_hash_prefix(next_name),
            false,
        ),
    ]
}

pub(super) fn rename_collection_prefixed_data(
    data_tx: &mut cntryl_midge::Transaction,
    current_name: &str,
    next_name: &str,
    relation_id: u64,
) -> Result<(), CassieError> {
    for (current_prefix, next_prefix, _is_normalized_vector) in
        collection_rename_prefixes(current_name, next_name, relation_id)
    {
        let documents = collect_scan(
            data_tx
                .scan(&Query::new().prefix(current_prefix.clone().into()))
                .map_err(CassieError::from)?,
        )?;
        let mut entries = Vec::new();
        for (key, value) in documents {
            entries.push((key, value));
        }

        for (key, value) in entries {
            let Some(id) = key.strip_prefix(current_prefix.as_slice()) else {
                continue;
            };
            let id = id.to_vec();
            let next_key = [next_prefix.as_slice(), id.as_slice()].concat();
            let next_value = value;
            data_tx.delete(key).map_err(CassieError::from)?;
            if data_tx.get(&next_key).map_err(CassieError::from)?.is_none() {
                data_tx
                    .put(next_key, next_value, None)
                    .map_err(CassieError::from)?;
            }
        }
    }
    Ok(())
}
