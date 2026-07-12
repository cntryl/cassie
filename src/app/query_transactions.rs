use crate::midge::adapter::DocumentWriteOp;
use std::collections::BTreeMap;

use super::{
    Cassie, CassieError, CassieSession, QueryResult, TransactionAction, TransactionRowChange,
    TransactionStatement,
};

impl Cassie {
    pub(super) fn execute_transaction_statement(
        &self,
        session: &CassieSession,
        statement: &TransactionStatement,
    ) -> Result<QueryResult, CassieError> {
        let command = match &statement.action {
            TransactionAction::Begin => {
                session.begin_transaction(statement.isolation)?;
                "BEGIN"
            }
            TransactionAction::Commit => {
                self.commit_transaction(session)?;
                "COMMIT"
            }
            TransactionAction::Rollback => {
                session.rollback_transaction();
                "ROLLBACK"
            }
            TransactionAction::Savepoint { name } => {
                session.create_savepoint(name)?;
                "SAVEPOINT"
            }
            TransactionAction::RollbackTo { name } => {
                session.rollback_to_savepoint(name)?;
                "ROLLBACK"
            }
            TransactionAction::Release { name } => {
                session.release_savepoint(name)?;
                "RELEASE"
            }
        };

        Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: command.to_string(),
        })
    }

    fn commit_transaction(&self, session: &CassieSession) -> Result<(), CassieError> {
        if session.is_transaction_failed() {
            return Err(CassieError::Execution(
                "transaction is failed; rollback required".to_string(),
            ));
        }
        let mut writes = BTreeMap::new();
        for (collection, collection_writes) in session.transaction_writes() {
            let write_ops = collection_writes
                .into_iter()
                .map(|(id, change)| match change {
                    TransactionRowChange::Upsert(payload) => DocumentWriteOp::Put { id, payload },
                    TransactionRowChange::Delete => DocumentWriteOp::Delete { id },
                })
                .collect::<Vec<_>>();
            if !write_ops.is_empty() {
                writes.insert(collection, write_ops);
            }
        }
        if writes.len() > 1 {
            return Err(CassieError::Unsupported(
                "transactions may modify only one collection".to_string(),
            ));
        }

        let mut changed_collections = Vec::new();
        if !writes.is_empty() {
            let collection = writes.keys().next().expect("non-empty writes");
            let options = self.document_write_options(collection);
            let reports = self
                .midge
                .apply_document_write_batches_with_options(&writes, &options)
                .inspect_err(|_| session.mark_transaction_failed())?;
            let mut latest_epoch = None;
            for (collection, report) in reports {
                self.runtime
                    .record_projection_write_batch(collection.clone(), &report.stats);
                if report.stats.row_puts > 0
                    || report.stats.row_deletes > 0
                    || report.stats.index_puts > 0
                    || report.stats.index_deletes > 0
                    || report.stats.metadata_puts > 0
                    || report.stats.metadata_deletes > 0
                    || report.stats.batch_flushes > 0
                {
                    changed_collections.push(collection);
                }
                latest_epoch = latest_epoch.or(report.data_epoch);
            }
            if let Some(epoch) = latest_epoch {
                self.runtime.set_data_epoch(epoch);
            }
        }
        changed_collections.sort();
        changed_collections.dedup();
        let controls = self.runtime.query_controls(std::time::Instant::now());
        for collection in changed_collections {
            crate::executor::refresh_rollups_for_source_external(self, &collection, &controls)
                .map_err(|error| CassieError::Execution(format!("{error:?}")))?;
            let _ = crate::executor::mark_source_projections_stale_external(self, &collection);
        }
        session.commit_transaction();
        Ok(())
    }
}
