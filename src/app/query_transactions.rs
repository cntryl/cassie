use crate::midge::adapter::DocumentWriteOp;

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
                if session.is_transaction_failed() {
                    return Err(CassieError::Execution(
                        "transaction is failed; rollback required".to_string(),
                    ));
                }
                let mut changed_collections = Vec::new();
                for (collection, writes) in session.transaction_writes() {
                    let mut write_ops = Vec::new();
                    for (id, change) in writes {
                        write_ops.push(match change {
                            TransactionRowChange::Upsert(payload) => {
                                DocumentWriteOp::Put { id, payload }
                            }
                            TransactionRowChange::Delete => DocumentWriteOp::Delete { id },
                        });
                    }

                    if write_ops.is_empty() {
                        continue;
                    }

                    let report = self
                        .midge
                        .apply_document_write_batch(&collection, write_ops)
                        .inspect_err(|_| {
                            session.mark_transaction_failed();
                        })?;
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
                        changed_collections.push(collection.clone());
                    }
                }

                changed_collections.sort();
                changed_collections.dedup();

                if !changed_collections.is_empty() {
                    let controls = self.runtime.query_controls(std::time::Instant::now());
                    for collection in changed_collections {
                        crate::executor::refresh_rollups_for_source_external(
                            self,
                            &collection,
                            &controls,
                        )
                        .map_err(|error| CassieError::Execution(format!("{error:?}")))?;
                    }
                }

                session.commit_transaction();
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
}
