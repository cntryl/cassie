//! Transaction ownership for a retained REPEATABLE READ data/catalog view.

use std::mem::size_of;

use super::{Arc, CassieError, CassieSession, SessionTransactionStatus, TransactionIsolation};
use crate::app::TransactionErrorKind;
use crate::catalog::ControlledCatalogSnapshot;
use crate::midge::adapter::{Midge, RetainedRowFence, RetainedRowHistory, StatementDataRead};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(super) struct TransactionReadSnapshot {
    fence: parking_lot::Mutex<Option<Box<RetainedRowFence>>>,
    history: Option<Arc<RetainedRowHistory>>,
    data: Arc<StatementDataRead>,
    catalog: Arc<ControlledCatalogSnapshot>,
    _memory: QueryMemoryReservation,
}

impl TransactionReadSnapshot {
    pub(super) fn catalog(&self) -> &crate::catalog::Catalog {
        self.catalog.catalog()
    }

    pub(super) fn catalog_owner(&self) -> Arc<ControlledCatalogSnapshot> {
        Arc::clone(&self.catalog)
    }
}

impl std::fmt::Debug for TransactionReadSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TransactionReadSnapshot")
            .field("data_epoch", &self.data.data_epoch())
            .field("catalog_version", &self.catalog.catalog().version())
            .finish_non_exhaustive()
    }
}

impl CassieSession {
    pub(super) fn repeatable_read_identity(&self) -> Option<usize> {
        self.transaction
            .lock()
            .repeatable_read
            .as_ref()
            .map(|owner| Arc::as_ptr(owner) as usize)
    }

    pub(crate) fn is_repeatable_read_write(&self) -> bool {
        let transaction = self.transaction.lock();
        transaction.isolation == Some(TransactionIsolation::RepeatableRead)
            && transaction.repeatable_read.as_ref().map_or_else(
                || {
                    transaction.modes.read_only == Some(false)
                        || transaction
                            .savepoints
                            .iter()
                            .any(|saved| saved.modes.read_only == Some(false))
                },
                |owner| owner.history.is_some(),
            )
    }

    pub(crate) fn repeatable_read_capture_generation(&self) -> Option<u64> {
        let transaction = self.transaction.lock();
        (transaction.status == SessionTransactionStatus::InTransaction
            && !transaction.implicit
            && transaction.isolation == Some(TransactionIsolation::RepeatableRead)
            && transaction.repeatable_read.is_none())
        .then_some(transaction.generation)
    }

    pub(crate) fn install_repeatable_read_snapshot(
        &self,
        generation: u64,
        data: Arc<StatementDataRead>,
        catalog: Arc<ControlledCatalogSnapshot>,
        fence: Option<Box<RetainedRowFence>>,
        history: Option<Arc<RetainedRowHistory>>,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        controls.check_interruption()?;
        let memory = controls
            .reserve_query_memory(size_of::<TransactionReadSnapshot>() + 2 * size_of::<usize>())?;
        let owner = Arc::new(TransactionReadSnapshot {
            fence: parking_lot::Mutex::new(fence),
            history,
            data,
            catalog,
            _memory: memory,
        });
        let mut transaction = self.transaction.lock();
        if transaction.status != SessionTransactionStatus::InTransaction
            || transaction.generation != generation
            || transaction.isolation != Some(TransactionIsolation::RepeatableRead)
        {
            return Err(CassieError::Transaction {
                kind: TransactionErrorKind::ActiveTransaction,
                message: "transaction changed while its read snapshot was acquired".to_owned(),
            });
        }
        if transaction.repeatable_read.is_none() {
            transaction.repeatable_read = Some(owner);
            // Parse/Bind snapshot acquisition also fixes the characteristics.
            transaction.has_query = true;
        }
        Ok(())
    }

    pub(crate) fn repeatable_conflict_history(
        &self,
        collection: &str,
        id: &str,
    ) -> Option<Arc<RetainedRowHistory>> {
        let transaction = self.transaction.lock();
        let owner = transaction.repeatable_read.as_ref()?;
        if transaction.writes.iter().any(|(name, changes)| {
            crate::catalog::name_matches(name, collection)
                && matches!(
                    changes.get(id),
                    Some(super::TransactionRowChange::Upsert(_))
                )
        }) {
            return None;
        }
        owner.history.as_ref().map(Arc::clone)
    }

    pub(crate) fn take_repeatable_row_fence(&self) -> Option<Box<RetainedRowFence>> {
        let transaction = self.transaction.lock();
        let owner = transaction.repeatable_read.as_ref()?;
        let fence = owner.fence.lock().take();
        fence
    }

    pub(crate) fn validate_repeatable_row_writes(
        &self,
        fence: Box<RetainedRowFence>,
        catalog: &crate::catalog::Catalog,
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        let transaction = self.transaction.lock();
        let rows = transaction.writes.iter().flat_map(|(collection, changes)| {
            let relation_id = catalog.collection_storage_id(collection);
            changes.keys().map(move |id| {
                relation_id
                    .filter(|identity| *identity != 0)
                    .map(|identity| (collection.as_str(), identity, id.as_str()))
                    .ok_or_else(|| {
                        CassieError::Unsupported(
                            "REPEATABLE READ row validation requires a persisted relation identity"
                                .to_owned(),
                        )
                    })
            })
        });
        fence.validate_rows(rows, controls)
    }

    pub(crate) fn repeatable_statement_read(
        &self,
        midge: &Arc<Midge>,
        database: &str,
        controls: &QueryExecutionControls,
    ) -> Result<Option<Arc<StatementDataRead>>, CassieError> {
        let transaction = self.transaction.lock();
        let Some(owner) = transaction.repeatable_read.as_ref() else {
            return Ok(None);
        };
        if !owner.data.matches_current_family(midge, database)? {
            return Err(CassieError::Transaction {
                kind: TransactionErrorKind::SerializationFailure,
                message: "database family changed during REPEATABLE READ".to_owned(),
            });
        }
        // Tightening future writes to READ ONLY does not discard a previously
        // captured RW transaction's own changes or its publication authority.
        if owner.history.is_none() {
            return Ok(Some(Arc::clone(&owner.data)));
        }
        let overlay = self.capture_statement_overlay_locked(&transaction, controls)?;
        StatementDataRead::with_overlay(&owner.data, Some(overlay), controls).map(Some)
    }

    /// The direct cursor path has already declined native streaming when this
    /// relation has own changes. It needs only the immutable committed view.
    pub(crate) fn repeatable_committed_read(
        &self,
        midge: &Arc<Midge>,
        database: &str,
    ) -> Result<Option<Arc<StatementDataRead>>, CassieError> {
        let transaction = self.transaction.lock();
        let Some(owner) = transaction.repeatable_read.as_ref() else {
            return Ok(None);
        };
        if !owner.data.matches_current_family(midge, database)? {
            return Err(CassieError::Transaction {
                kind: TransactionErrorKind::SerializationFailure,
                message: "database family changed during REPEATABLE READ".to_owned(),
            });
        }
        Ok(Some(Arc::clone(&owner.data)))
    }

    pub(crate) fn transaction_index_visible(
        &self,
        index: &crate::catalog::IndexMeta,
        current: &crate::catalog::Catalog,
    ) -> bool {
        let transaction = self.transaction.lock();
        transaction.repeatable_read.as_ref().is_none_or(|owner| {
            owner
                .catalog
                .catalog()
                .contains_index_read_identity(current, index)
        })
    }

    pub(crate) fn transaction_catalog_snapshot(&self) -> Option<Arc<ControlledCatalogSnapshot>> {
        self.transaction
            .lock()
            .repeatable_read
            .as_ref()
            .map(|owner| Arc::clone(&owner.catalog))
    }
}
