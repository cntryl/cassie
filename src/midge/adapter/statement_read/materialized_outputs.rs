//! Retain only newly created, transaction-private materialized outputs for a
//! repeatable-read command. Other relations continue to use its committed Data owner.
use super::{
    Arc, CassieError, DescriptorAuthority, Midge, QueryExecutionControls, QueryMemoryReservation,
    StatementDataRead, StatementDataSnapshot, StatementReadScope, TransactionMode,
};
use crate::executor::retained_memory::add;
use crate::midge::adapter::{
    key_encoding as key, transactions::ExpectedStorageValue, AdapterTransaction, StorageOverlay,
};

pub(super) struct PrivateOutputs {
    names: Vec<String>,
    read: Arc<StatementDataRead>,
    memory: QueryMemoryReservation,
}

impl StatementDataRead {
    pub(crate) fn with_materialized_outputs(
        retained: &Self,
        overlay: Arc<crate::app::StatementOverlay>,
        storage: &StorageOverlay,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<Self>, CassieError> {
        let mut owner = Self::with_overlay(retained, Some(overlay), controls)?;
        let Some(private) = PrivateOutputs::capture(retained, storage, controls)? else {
            return Ok(owner);
        };
        Arc::get_mut(&mut owner)
            .expect("new command owner")
            .private_outputs = Some(Arc::new(private));
        Ok(owner)
    }

    pub(crate) fn enter_materialized_output(
        controls: &QueryExecutionControls,
        output: &str,
    ) -> Option<StatementReadScope> {
        let outputs = controls.statement_read()?.private_outputs.as_ref()?;
        outputs
            .names
            .iter()
            .any(|name| name == output)
            .then(|| StatementReadScope::enter(Some(&outputs.read)))
    }
}

impl PrivateOutputs {
    pub(super) fn accounted_bytes(&self) -> Result<usize, CassieError> {
        add(
            self.memory.bytes(),
            add(self.read.memory.bytes(), self.read.snapshot._memory.bytes())?,
        )
    }

    fn capture(
        retained: &StatementDataRead,
        storage: &StorageOverlay,
        controls: &QueryExecutionControls,
    ) -> Result<Option<Self>, CassieError> {
        if storage.changes.is_empty() {
            return Ok(None);
        }
        let projection_prefix = key::projection_prefix();
        let mut memory = controls.reserve_query_memory(0)?;
        let mut names = Vec::new();
        let mut native = None;
        let snapshot = &retained.snapshot;
        for (key, change) in storage
            .changes
            .range(projection_prefix.clone()..)
            .take_while(|(key, _)| key.starts_with(&projection_prefix))
        {
            controls.check_interruption()?;
            let Some(value) = &change.value else {
                continue;
            };
            let _scratch = controls
                .reserve_query_memory(crate::executor::retained_memory::mul(value.len(), 8)?)?;
            let projection: crate::catalog::ProjectionMeta = serde_json::from_slice(value)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            let Some(output) = projection.active_output_collection().filter(|_| {
                projection.materialized.as_ref().is_some_and(
                    crate::catalog::MaterializedProjectionMeta::is_sql_materialized_view,
                )
            }) else {
                continue;
            };
            if *key != Midge::projection_key(&projection.collection) {
                continue;
            }
            let metadata_key = key::collection_metadata_key(output);
            let Some(change) = storage.changes.get(&metadata_key) else {
                continue;
            };
            if !matches!(change.expected, ExpectedStorageValue::Missing) {
                continue;
            }
            let Some(metadata) = &change.value else {
                continue;
            };
            let relation: crate::catalog::CollectionMeta = serde_json::from_slice(metadata)
                .map_err(|error| CassieError::Parse(error.to_string()))?;
            if relation.name != output || relation.storage_id == 0 {
                continue;
            }
            let tx = match &mut native {
                Some(tx) => tx,
                slot @ None => slot.insert(
                    snapshot
                        .midge
                        .engine
                        .begin_tx(snapshot.family_id, TransactionMode::ReadWrite)?,
                ),
            };
            copy_output(storage, &relation, tx, controls, &mut memory)?;
            memory.try_grow(add(output.len(), 2 * size_of::<String>())?)?;
            names.push(output.to_owned());
        }
        let Some(native) = native else {
            return Ok(None);
        };
        let snapshot_memory = controls.reserve_query_memory(add(
            size_of::<StatementDataSnapshot>() + 2 * size_of::<usize>(),
            snapshot.database.len(),
        )?)?;
        let read = Arc::new(StatementDataRead {
            snapshot: Arc::new(StatementDataSnapshot {
                tx: AdapterTransaction::raw(native),
                midge: Arc::clone(&snapshot.midge),
                database: snapshot.database.clone(),
                family_id: snapshot.family_id,
                data_epoch: snapshot.data_epoch,
                descriptor_authority: DescriptorAuthority::StatementSnapshot,
                _memory: snapshot_memory,
            }),
            overlay: None,
            private_outputs: None,
            memory: controls
                .reserve_query_memory(size_of::<StatementDataRead>() + 2 * size_of::<usize>())?,
        });
        memory.try_grow(size_of::<Self>() + 2 * size_of::<usize>())?;
        Ok(Some(Self {
            names,
            read,
            memory,
        }))
    }
}

fn copy_output(
    storage: &StorageOverlay,
    relation: &crate::catalog::CollectionMeta,
    tx: &mut cntryl_midge::Transaction,
    controls: &QueryExecutionControls,
    memory: &mut QueryMemoryReservation,
) -> Result<(), CassieError> {
    let output = &relation.name;
    // A private build creates a fresh durable relation ID. Copy only its
    // descriptors, row blobs and scalar keys, never an unrelated current row.
    for prefix in [
        key::collection_metadata_key(output),
        key::collection_schema_key(output),
        key::row_schema_key(output),
        key::index_collection_prefix(output),
        key::row_prefix(relation.storage_id),
        key::scalar_index_collection_prefix(relation.storage_id),
    ] {
        for (key, change) in storage
            .changes
            .range(prefix.clone()..)
            .take_while(|(key, _)| key.starts_with(&prefix))
        {
            controls.check_interruption()?;
            memory.try_grow(add(
                256,
                add(key.len(), change.value.as_ref().map_or(0, Vec::len))?,
            )?)?;
            if let Some(value) = &change.value {
                tx.put(key.clone(), value.clone(), None)?;
            } else {
                tx.delete(key.clone())?;
            }
        }
    }
    Ok(())
}
