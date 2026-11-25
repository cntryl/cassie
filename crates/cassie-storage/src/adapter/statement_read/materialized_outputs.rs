//! Copies transaction-private materialized rows into an opaque native snapshot.

use super::{
    CassieError, DescriptorAuthority, Midge, NativeStatementSnapshot, StorageRequestContext,
};
use crate::adapter::{
    key_encoding as key, transactions::ExpectedStorageValue, AdapterTransaction, StorageOverlay,
};
use cassie_core::request::accounted::retained::add;
use cassie_core::request::RequestMemoryReservation;
use std::mem::size_of;
use std::sync::Arc;

impl Midge {
    /// Copies a newly-created relation from a transaction overlay into a native read owner.
    ///
    /// # Errors
    /// Returns an error if the private output snapshot cannot be captured or admitted.
    pub fn capture_private_materialized_snapshot(
        self: &Arc<Self>,
        storage: &StorageOverlay,
        database: &str,
        output: &str,
        context: &StorageRequestContext<'_>,
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
            .begin_tx(family_id, super::super::TransactionMode::ReadWrite)?;
        let mut memory = context.reserve(1024)?;
        copy_output(storage, &relation, &mut native, context, &mut memory)?;
        let native = AdapterTransaction::raw(native, self.value_hooks());
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
    context: &StorageRequestContext<'_>,
    memory: &mut RequestMemoryReservation,
) -> Result<(), CassieError> {
    let output = &relation.name;
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
