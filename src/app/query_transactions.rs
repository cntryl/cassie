use super::{
    Cassie, CassieError, CassieSession, QueryResult, QueryStatement, TransactionAction,
    TransactionErrorKind, TransactionStatement,
};

impl CassieError {
    /// `PostgreSQL` 25P02 for work attempted inside a failed transaction block.
    pub(crate) fn failed_transaction() -> Self {
        Self::Transaction {
            kind: TransactionErrorKind::FailedTransaction,
            message:
                "current transaction is aborted, commands ignored until end of transaction block"
                    .to_owned(),
        }
    }

    /// `PostgreSQL` 25P01 for a savepoint control outside a transaction block.
    pub(crate) fn savepoint_outside_transaction(command: &str) -> Self {
        Self::Transaction {
            kind: TransactionErrorKind::NoActiveTransaction,
            message: format!("{command} can only be used in transaction blocks"),
        }
    }

    /// `PostgreSQL` 3B001 for a savepoint name absent from the current block.
    pub(crate) fn missing_savepoint(name: &str) -> Self {
        Self::Transaction {
            kind: TransactionErrorKind::InvalidSavepoint,
            message: format!("savepoint \"{name}\" does not exist"),
        }
    }
}

impl Cassie {
    pub(crate) fn ensure_transaction_recovery(
        session: &CassieSession,
        parsed: &crate::sql::ast::ParsedStatement,
    ) -> Result<(), CassieError> {
        let recovery = matches!(
            &parsed.statement,
            QueryStatement::Transaction(TransactionStatement {
                action: TransactionAction::Commit
                    | TransactionAction::Rollback
                    | TransactionAction::RollbackTo { .. },
                ..
            })
        );
        if session.is_transaction_failed() && !recovery {
            return Err(CassieError::failed_transaction());
        }
        Ok(())
    }

    pub(super) fn execute_transaction_statement(
        &self,
        session: &CassieSession,
        statement: &TransactionStatement,
        controls: &crate::runtime::QueryExecutionControls,
    ) -> Result<QueryResult, CassieError> {
        let savepoint_control = match statement.action {
            TransactionAction::Savepoint { .. } => Some("SAVEPOINT"),
            TransactionAction::RollbackTo { .. } => Some("ROLLBACK TO SAVEPOINT"),
            TransactionAction::Release { .. } => Some("RELEASE SAVEPOINT"),
            _ => None,
        };
        if let Some(control) = savepoint_control.filter(|_| session.is_implicit_transaction()) {
            session.mark_transaction_failed();
            return Err(CassieError::savepoint_outside_transaction(control));
        }
        // PostgreSQL ends a failed block on COMMIT by rolling it back.
        if statement.chain
            && (!session.is_transaction_active() && !session.is_transaction_failed()
                || session.is_implicit_transaction())
        {
            return Err(CassieError::Transaction {
                kind: TransactionErrorKind::NoActiveTransaction,
                message: "AND CHAIN can only be used in transaction blocks".to_string(),
            });
        }
        let characteristics = statement.chain.then(|| {
            if session.is_transaction_failed() {
                (None, crate::sql::ast::TransactionModes::default())
            } else {
                session.transaction_characteristics()
            }
        });
        let failed_commit = matches!(statement.action, TransactionAction::Commit)
            && session.is_transaction_failed();
        let command = match &statement.action {
            TransactionAction::Begin => "BEGIN",
            TransactionAction::Commit if failed_commit => "ROLLBACK",
            TransactionAction::Commit => "COMMIT",
            TransactionAction::Rollback | TransactionAction::RollbackTo { .. } => "ROLLBACK",
            TransactionAction::Savepoint { .. } => "SAVEPOINT",
            TransactionAction::Release { .. } => "RELEASE",
        };
        let mut result = QueryResult::try_builder(controls)?;
        result.try_set_command(command)?;
        let output = result.finish()?;

        match &statement.action {
            TransactionAction::Begin => {
                session.begin_transaction_with_modes(statement.isolation, statement.modes)?;
            }
            TransactionAction::Commit if failed_commit => session.rollback_transaction(),
            TransactionAction::Commit => self.commit_transaction(session)?,
            TransactionAction::Rollback => session.rollback_transaction(),
            TransactionAction::Savepoint { name } => session.create_savepoint(name)?,
            TransactionAction::RollbackTo { name } => session.rollback_to_savepoint(name)?,
            TransactionAction::Release { name } => session.release_savepoint(name)?,
        }
        if let Some((isolation, modes)) = characteristics {
            session.begin_transaction_with_modes(isolation, modes)?;
        }
        Ok(output)
    }

    pub(crate) fn commit_transaction(&self, session: &CassieSession) -> Result<(), CassieError> {
        if session.is_transaction_failed() {
            return Err(CassieError::failed_transaction());
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

#[cfg(test)]
#[path = "query_transactions/output_admission_tests.rs"]
mod output_admission_tests;
