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

#[derive(Debug)]
pub(crate) struct PrivateOutput {
    name: String,
    read: Arc<StatementDataRead>,
    memory: QueryMemoryReservation,
}

/// Keyed by logical relation. Older generations live only while a savepoint or a
/// previously bound command still owns its directory.
pub(crate) type PrivateOutputs = std::collections::BTreeMap<String, Arc<PrivateOutput>>;

impl StatementDataRead {
    pub(crate) fn with_materialized_outputs(
        retained: &Self,
        overlay: Arc<crate::app::StatementOverlay>,
        storage: &StorageOverlay,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<Self>, CassieError> {
        let mut owner = Self::with_overlay(retained, Some(overlay), controls)?;
        Arc::get_mut(&mut owner)
            .expect("new command owner")
            .private_outputs
            .clone_from(&storage.private_outputs);
        Ok(owner)
    }

    pub(crate) fn enter_materialized_output(
        controls: &QueryExecutionControls,
        logical: &str,
        output: &str,
    ) -> Option<StatementReadScope> {
        let private = controls
            .statement_read()?
            .private_outputs
            .as_ref()?
            .get(logical)?;
        (private.name == output).then(|| StatementReadScope::enter(Some(&private.read)))
    }
}

impl PrivateOutput {
    pub(in crate::midge::adapter) fn accounted_bytes(&self) -> Result<usize, CassieError> {
        add(
            self.memory.bytes(),
            add(self.read.memory.bytes(), self.read.snapshot._memory.bytes())?,
        )
    }
}

impl Midge {
    /// A refreshed output is immutable. Build its native read owner once, while
    /// the transaction owns the new physical relation and its completed indexes.
    pub(crate) fn retain_private_materialized_output(
        self: &Arc<Self>,
        session: Option<&crate::app::CassieSession>,
        logical: &str,
        output: &str,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let Some(session) = session.filter(|s| s.is_repeatable_read_write()) else {
            return Ok(());
        };
        let overlay = session.transaction_storage_overlay()?;
        let mut storage = overlay.lock();
        let Some(change) = storage.changes.get(&key::collection_metadata_key(output)) else {
            return Ok(());
        };
        let Some(metadata) = &change.value else {
            return Ok(());
        };
        if !matches!(change.expected, ExpectedStorageValue::Missing) {
            return Ok(());
        }
        let relation: crate::catalog::CollectionMeta =
            serde_json::from_slice(metadata).map_err(|e| CassieError::Parse(e.to_string()))?;
        if relation.name != output || relation.storage_id == 0 {
            return Ok(());
        }
        let database = session.current_database().unwrap_or(&self.default_database);
        let family_id = self.database_family(database)?.id();
        let mut native = self
            .engine
            .begin_tx(family_id, TransactionMode::ReadWrite)?;
        let mut memory = controls.reserve_query_memory(add(
            1024 + size_of::<PrivateOutput>(),
            add(output.len(), logical.len())?,
        )?)?;
        copy_output(&storage, &relation, &mut native, controls, &mut memory)?;
        let native = AdapterTransaction::raw(native);
        let read = Arc::new(StatementDataRead {
            snapshot: Arc::new(StatementDataSnapshot {
                data_epoch: Self::load_data_epoch_from_tx(&native)?,
                tx: native,
                midge: Arc::clone(self),
                database: database.to_owned(),
                family_id,
                descriptor_authority: DescriptorAuthority::StatementSnapshot,
                _memory: controls.reserve_query_memory(add(
                    size_of::<StatementDataSnapshot>() + 2 * size_of::<usize>(),
                    database.len(),
                )?)?,
            }),
            overlay: None,
            private_outputs: None,
            memory: controls
                .reserve_query_memory(size_of::<StatementDataRead>() + 2 * size_of::<usize>())?,
        });
        let next = PrivateOutput {
            name: output.to_owned(),
            read,
            memory,
        };
        Arc::make_mut(storage.private_outputs.get_or_insert_with(Arc::default))
            .insert(logical.to_owned(), Arc::new(next));
        Ok(())
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
