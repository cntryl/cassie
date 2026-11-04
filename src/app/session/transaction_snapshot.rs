//! Transaction ownership for a retained REPEATABLE READ data/catalog view.

use std::mem::size_of;

use super::{Arc, CassieError, CassieSession, SessionTransactionStatus, TransactionIsolation};
use crate::app::TransactionErrorKind;
use crate::catalog::ControlledCatalogSnapshot;
use crate::midge::adapter::{Midge, StatementDataRead};
use crate::runtime::{QueryExecutionControls, QueryMemoryReservation};

pub(super) struct TransactionReadSnapshot {
    data: Arc<StatementDataRead>,
    catalog: Arc<ControlledCatalogSnapshot>,
    _memory: QueryMemoryReservation,
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
        controls: &QueryExecutionControls,
    ) -> Result<(), CassieError> {
        controls.check_interruption()?;
        let memory = controls
            .reserve_query_memory(size_of::<TransactionReadSnapshot>() + 2 * size_of::<usize>())?;
        let owner = Arc::new(TransactionReadSnapshot {
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

    pub(crate) fn repeatable_statement_read(
        &self,
        midge: &Arc<Midge>,
        database: &str,
    ) -> Result<Option<Arc<StatementDataRead>>, CassieError> {
        let owner = self.transaction.lock().repeatable_read.clone();
        let Some(owner) = owner else {
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
