//! Retain only newly created, transaction-private materialized outputs for a
//! repeatable-read command. Other relations continue to use its committed Data owner.
use super::{
    Arc, CassieError, DescriptorAuthority, Midge, NativeStatementReadContext,
    NativeStatementSnapshot, QueryExecutionControls, StatementDataRead, StatementReadScope,
    TransactionMode,
};
use crate::app::PrivateOutputs;
use crate::executor::retained_memory::add;
use crate::midge::adapter::{
    key_encoding as key, transactions::ExpectedStorageValue, AdapterTransaction, StorageOverlay,
};

#[derive(Clone)]
pub(super) enum MaterializedReads {
    All(Arc<PrivateOutputs>),
    One(Arc<PrivateOutput>),
}

impl MaterializedReads {
    fn get(&self, logical: &str) -> Option<&Arc<PrivateOutput>> {
        match self {
            Self::All(outputs) => outputs.get(logical),
            Self::One(selected) => Some(selected),
        }
    }

    pub(super) fn accounted_bytes(&self) -> Result<usize, CassieError> {
        match self {
            Self::All(outputs) => outputs
                .values()
                .try_fold(0, |sum, output| add(sum, output.accounted_bytes()?)),
            Self::One(output) => output.accounted_bytes(),
        }
    }
}

impl StatementDataRead {
    pub(crate) fn retain_bound_output(
        owner: &mut Arc<Self>,
        logical: &str,
        private_outputs: Option<&Arc<PrivateOutputs>>,
    ) {
        if let Some(outputs) = private_outputs {
            // Only a freshly captured, writable RR command has private outputs.
            let data = Arc::get_mut(owner).expect("new private command owner");
            data.private_outputs = outputs.get(logical).cloned().map(MaterializedReads::One);
        }
    }

    pub(crate) fn with_materialized_outputs(
        retained: &Self,
        overlay: Arc<crate::app::StatementOverlay>,
        private_outputs: Option<&Arc<PrivateOutputs>>,
        controls: &QueryExecutionControls,
    ) -> Result<Arc<Self>, CassieError> {
        let mut owner = Self::with_overlay(retained, Some(overlay), controls)?;
        Arc::get_mut(&mut owner)
            .expect("new command owner")
            .private_outputs =
            private_outputs.map(|outputs| MaterializedReads::All(Arc::clone(outputs)));
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

impl Midge {
    /// Copies the transaction's newly-created relation into an immutable native
    /// read owner. Session registry and command wrapper ownership stay in engine.
    pub(crate) fn capture_private_materialized_snapshot(
        self: &Arc<Self>,
        storage: &StorageOverlay,
        database: &str,
        output: &str,
        context: &NativeStatementReadContext<'_>,
    ) -> Result<Option<Arc<NativeStatementSnapshot>>, CassieError> {
        let Some(change) = storage.changes.get(&key::collection_metadata_key(output)) else {
            return Ok(None);
        };
        let Some(metadata) = &change.value else {
            return Ok(None);
        };
        if !matches!(change.expected, ExpectedStorageValue::Missing) {
            return Ok(None);
        }
        let relation: crate::catalog::CollectionMeta =
            serde_json::from_slice(metadata).map_err(|e| CassieError::Parse(e.to_string()))?;
        if relation.name != output || relation.storage_id == 0 {
            return Ok(None);
        }
        let family_id = self.database_family(database)?.id();
        context.check_interruption()?;
        let mut native = self
            .engine
            .begin_tx(family_id, TransactionMode::ReadWrite)?;
        let mut memory = context.reserve(1024)?;
        copy_output(storage, &relation, &mut native, context, &mut memory)?;
        let native = AdapterTransaction::raw(native);
        let mut retained_memory = context.reserve(add(
            size_of::<NativeStatementSnapshot>() + 2 * size_of::<usize>(),
            database.len(),
        )?)?;
        retained_memory.absorb(memory);
        Ok(Some(Arc::new(NativeStatementSnapshot {
            data_epoch: Self::load_data_epoch_from_tx(&native)?,
            tx: native,
            midge: Arc::clone(self),
            database: database.to_owned(),
            family_id,
            descriptor_authority: DescriptorAuthority::StatementSnapshot,
            memory: retained_memory,
        })))
    }
}

fn copy_output(
    storage: &StorageOverlay,
    relation: &crate::catalog::CollectionMeta,
    tx: &mut cntryl_midge::Transaction,
    context: &NativeStatementReadContext<'_>,
    memory: &mut cassie_core::request::RequestMemoryReservation,
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
            context.check_interruption()?;
            context.grow(
                memory,
                add(
                    256,
                    add(key.len(), change.value.as_ref().map_or(0, Vec::len))?,
                )?,
            )?;
            if let Some(value) = &change.value {
                tx.put(key.clone(), value.clone(), None)?;
            } else {
                tx.delete(key.clone())?;
            }
        }
    }
    Ok(())
}
