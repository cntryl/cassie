//! Current descriptor ownership around a repeatable-write commit attempt.

use std::time::Duration;

use super::{Cassie, CassieError};
use crate::runtime::QueryExecutionControls;

pub(super) enum CatalogCommitGuard<'a> {
    Read(parking_lot::RecursiveRwLockReadGuard<'a, ()>),
    Write(parking_lot::RecursiveRwLockWriteGuard<'a, ()>),
}

impl CatalogCommitGuard<'_> {
    pub(super) fn retire(self) {
        match self {
            Self::Read(guard) => drop(guard),
            Self::Write(guard) => drop(guard),
        }
    }
}

impl Cassie {
    pub(super) fn acquire_commit_catalog_guard<'a>(
        &'a self,
        write: bool,
        repeatable_write: bool,
        held: Option<parking_lot::RecursiveRwLockWriteGuard<'a, ()>>,
        controls: &QueryExecutionControls,
    ) -> Result<Option<CatalogCommitGuard<'a>>, CassieError> {
        if let Some(held) = held {
            return Ok(Some(CatalogCommitGuard::Write(held)));
        }
        if !write && !repeatable_write {
            return Ok(None);
        }
        loop {
            crate::midge::adapter::PublicationControlScope::check_interruption(controls)?;
            self.check_storage_recovery_required()?;
            let guard = if write {
                self.catalog_publication
                    .try_write_for(Duration::from_millis(2))
                    .map(CatalogCommitGuard::Write)
            } else {
                self.catalog_publication
                    .try_read_for(Duration::from_millis(2))
                    .map(CatalogCommitGuard::Read)
            };
            if let Some(guard) = guard {
                crate::midge::adapter::PublicationControlScope::check_interruption(controls)?;
                return Ok(Some(guard));
            }
        }
    }

    pub(super) fn publish_transaction_rows(
        &self,
        session: &super::CassieSession,
        database: &str,
        catalog: Option<&crate::catalog::Catalog>,
        collections: &[String],
        controls: &QueryExecutionControls,
        transactional_ddl: Option<(&str, &crate::midge::adapter::StorageOverlay)>,
    ) -> Result<crate::app::statement_mutation::CommittedWriteBatch, CassieError> {
        let repeatable_write = session.is_repeatable_read_write();
        let operation = || {
            if repeatable_write {
                match session.take_repeatable_row_fence() {
                    Some(fence) => session.validate_repeatable_row_writes(
                        fence,
                        catalog.unwrap_or(&self.publication_catalog),
                        controls,
                    )?,
                    None if !collections.is_empty() || catalog.is_some() => {
                        return Err(CassieError::Unsupported(
                            "REPEATABLE READ publication requires its retained row fence"
                                .to_owned(),
                        ));
                    }
                    None => {}
                }
            }
            if catalog.is_some() {
                self.bump_transaction_schema_epoch()?;
            }
            self.apply_staged_write_batches(session, Some(controls), transactional_ddl)
        };
        if repeatable_write {
            self.midge
                .with_database_publication(database, controls, operation)
        } else {
            operation()
        }
    }
}
