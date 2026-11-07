//! Session-owned catalog changes and admitted immutable read compositions.

use super::{Arc, CassieError, CassieSession, Catalog, Mutex, StorageOverlay};
use crate::catalog::ControlledCatalogSnapshot;
use crate::runtime::QueryExecutionControls;

#[derive(Debug, Clone, Default)]
pub(super) struct TransactionCatalogViews {
    base: Option<Arc<ControlledCatalogSnapshot>>,
    sql: Option<(u64, Arc<ControlledCatalogSnapshot>)>,
    descriptors: Option<((u64, u64), Arc<ControlledCatalogSnapshot>)>,
}

impl CassieSession {
    pub(crate) fn transaction_catalog_overlay(&self) -> Option<Catalog> {
        self.transaction.lock().catalog_overlay.clone()
    }

    pub(crate) fn transaction_catalog_version(&self) -> Option<u64> {
        self.transaction
            .lock()
            .catalog_overlay
            .as_ref()
            .map(Catalog::version)
    }

    /// The caller holds the current catalog publication guard. Compare all
    /// published metadata before binding, including no-op DDL dependencies.
    pub(crate) fn check_repeatable_catalog_write(
        &self,
        current: &Catalog,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let transaction = self.transaction.lock();
        let Some(owner) = transaction.repeatable_read.as_ref() else {
            return Ok(());
        };
        if transaction.modes.read_only == Some(false)
            && !current.matches_transaction_metadata(owner.catalog(), controls)?
        {
            return Err(CassieError::Transaction {
                kind: crate::app::TransactionErrorKind::SerializationFailure,
                message: "catalog metadata changed during REPEATABLE READ DDL".to_owned(),
            });
        }
        Ok(())
    }

    pub(crate) fn begin_transaction_catalog_overlay(
        &self,
        catalog: &Catalog,
        controls: &QueryExecutionControls,
    ) -> Result<Catalog, CassieError> {
        let mut transaction = self.transaction.lock();
        if transaction.catalog_overlay.is_none() {
            // Preserve an immutable admitted base for the semantic own delta;
            // the existing mutable DDL overlay must never mutate this owner.
            let base = transaction
                .repeatable_read
                .as_ref()
                .map(|_| catalog.snapshot_with_controls(controls))
                .transpose()?;
            transaction.catalog_base_version = Some(catalog.version());
            transaction.catalog_overlay = Some(catalog.detached_transaction_snapshot());
            transaction.catalog_views.base = base;
        }
        Ok(transaction
            .catalog_overlay
            .as_ref()
            .expect("initialized above")
            .clone())
    }

    /// SQL metadata uses the initial transaction view plus only own DDL changes.
    pub(crate) fn command_catalog_snapshot(
        &self,
        controls: &QueryExecutionControls,
    ) -> Result<Option<Arc<ControlledCatalogSnapshot>>, CassieError> {
        let mut transaction = self.transaction.lock();
        let Some(initial) = transaction
            .repeatable_read
            .as_ref()
            .map(|owner| owner.catalog_owner())
        else {
            return Ok(None);
        };
        let (Some(base), Some(overlay)) = (
            &transaction.catalog_views.base,
            &transaction.catalog_overlay,
        ) else {
            return Ok(Some(initial));
        };
        let version = overlay.version();
        if let Some((cached_version, cached)) = &transaction.catalog_views.sql {
            if *cached_version == version {
                return Ok(Some(Arc::clone(cached)));
            }
        }
        let merged = initial
            .catalog()
            .snapshot_with_delta(base.catalog(), overlay, controls)?;
        transaction.catalog_views.sql = Some((version, Arc::clone(&merged)));
        Ok(Some(merged))
    }

    /// Ordinary descriptor/authorization lookup stays current and includes own
    /// DDL. Keep this owner alive for every Catalog alias used by the command.
    pub(crate) fn command_descriptor_snapshot(
        &self,
        current: &Catalog,
        controls: &QueryExecutionControls,
    ) -> Result<Option<Arc<ControlledCatalogSnapshot>>, CassieError> {
        let mut transaction = self.transaction.lock();
        Self::command_descriptor_snapshot_locked(&mut transaction, current, controls)
    }

    pub(super) fn command_descriptor_snapshot_locked(
        transaction: &mut super::SessionTransactionState,
        current: &Catalog,
        controls: &QueryExecutionControls,
    ) -> Result<Option<Arc<ControlledCatalogSnapshot>>, CassieError> {
        let (Some(base), Some(overlay)) = (
            &transaction.catalog_views.base,
            &transaction.catalog_overlay,
        ) else {
            return Ok(None);
        };
        let version = (current.version(), overlay.version());
        if let Some((cached_version, cached)) = &transaction.catalog_views.descriptors {
            if *cached_version == version {
                return Ok(Some(Arc::clone(cached)));
            }
        }
        let merged = current.snapshot_with_delta(base.catalog(), overlay, controls)?;
        transaction.catalog_views.descriptors = Some((version, Arc::clone(&merged)));
        Ok(Some(merged))
    }

    pub(crate) fn transaction_catalog_base_version(&self) -> Option<u64> {
        self.transaction.lock().catalog_base_version
    }

    pub(crate) fn prune_transaction_sequence_cursors(
        &self,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let transaction = self.transaction.lock();
        let overlay = transaction.storage_overlay.lock();
        overlay.prune_sequence_cursors_with(
            transaction
                .savepoints
                .iter()
                .map(|savepoint| &savepoint.storage_overlay),
            controls,
        )
    }

    pub(crate) fn transaction_storage_overlay(&self) -> Arc<Mutex<StorageOverlay>> {
        Arc::clone(&self.transaction.lock().storage_overlay)
    }

    pub(crate) fn transaction_storage_overlay_snapshot(&self) -> StorageOverlay {
        self.transaction.lock().storage_overlay.lock().clone()
    }
}

#[cfg(test)]
mod tests;
