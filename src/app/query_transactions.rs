use super::{
    Cassie, CassieError, CassieSession, QueryResult, QueryStatement, TransactionAction,
    TransactionStatement,
};

impl Cassie {
    pub(crate) fn ensure_transaction_recovery(
        session: &CassieSession,
        parsed: &crate::sql::ast::ParsedStatement,
    ) -> Result<(), CassieError> {
        let recovery = matches!(
            &parsed.statement,
            QueryStatement::Transaction(TransactionStatement {
                action: TransactionAction::Rollback | TransactionAction::RollbackTo { .. },
                ..
            })
        );
        if session.is_transaction_failed() && !recovery {
            return Err(CassieError::Execution(
                "transaction is failed; rollback required".to_string(),
            ));
        }
        Ok(())
    }

    pub(super) fn execute_transaction_statement(
        &self,
        session: &CassieSession,
        statement: &TransactionStatement,
    ) -> Result<QueryResult, CassieError> {
        if session.is_implicit_transaction()
            && matches!(
                statement.action,
                TransactionAction::Savepoint { .. }
                    | TransactionAction::RollbackTo { .. }
                    | TransactionAction::Release { .. }
            )
        {
            session.mark_transaction_failed();
            return Err(CassieError::Unsupported(
                "savepoints require an explicit transaction".to_owned(),
            ));
        }
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

    pub(crate) fn commit_transaction(&self, session: &CassieSession) -> Result<(), CassieError> {
        if session.is_transaction_failed() {
            return Err(CassieError::Execution(
                "transaction is failed; rollback required".to_string(),
            ));
        }
        if !session.is_transaction_active() {
            return Err(CassieError::Execution(
                "COMMIT requires an active transaction".to_string(),
            ));
        }
        let (collections, referential) = self.transaction_commit_write_gates(session);
        let committed = if referential {
            self.midge.with_collection_write_gates(&collections, || {
                self.validate_staged_foreign_keys(session)?;
                self.validate_staged_parent_references(session)?;
                self.apply_staged_write_batches(session, None)
            })
        } else {
            self.midge.with_collection_gates(&collections, || {
                self.apply_staged_write_batches(session, None)
            })
        }
        .inspect_err(|_| session.mark_transaction_failed())?;

        session.commit_transaction();
        self.finish_staged_write_batches(committed, None);
        Ok(())
    }
}
