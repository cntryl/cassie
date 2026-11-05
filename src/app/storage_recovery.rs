//! Terminal storage recovery admission; cleanup never attempts publication.

use super::{
    Cassie, CassieError, CassieSession, QueryExecutionControls, QueryResult, QueryStatement,
    TransactionAction,
};

impl Cassie {
    pub(crate) fn check_storage_recovery_required(&self) -> Result<(), CassieError> {
        self.midge.check_storage_recovery_required()
    }

    pub(crate) fn check_storage_statement_admission(
        &self,
        statement: &QueryStatement,
    ) -> Result<(), CassieError> {
        if matches!(statement, QueryStatement::Transaction(transaction) if matches!(transaction.action, TransactionAction::Commit | TransactionAction::Rollback))
        {
            return Ok(());
        }
        self.check_storage_recovery_required()
    }

    pub(super) fn quarantined_sql_result(
        &self,
        session: &CassieSession,
        sql: &str,
        controls: &QueryExecutionControls,
    ) -> Option<Result<QueryResult, CassieError>> {
        let error = self.check_storage_recovery_required().err()?;
        Some(
            if let Some(statement) = crate::sql::parser::parse_terminal_transaction_control(sql) {
                Self::quarantined_end_result(session, &statement, controls, error)
            } else {
                Err(error)
            },
        )
    }

    pub(super) fn quarantined_statement_result(
        &self,
        session: &CassieSession,
        statement: &QueryStatement,
        controls: &QueryExecutionControls,
    ) -> Option<Result<QueryResult, CassieError>> {
        let error = self.check_storage_recovery_required().err()?;
        Some(if let QueryStatement::Transaction(statement) = statement {
            Self::quarantined_end_result(session, statement, controls, error)
        } else {
            Err(error)
        })
    }

    fn quarantined_end_result(
        session: &CassieSession,
        statement: &crate::sql::ast::TransactionStatement,
        controls: &QueryExecutionControls,
        error: CassieError,
    ) -> Result<QueryResult, CassieError> {
        if matches!(
            statement.action,
            TransactionAction::Commit | TransactionAction::Rollback
        ) {
            session.rollback_transaction();
            if matches!(statement.action, TransactionAction::Rollback) && !statement.chain {
                let mut result = QueryResult::try_builder(controls)?;
                result.try_set_command("ROLLBACK")?;
                return result.finish();
            }
        }
        Err(error)
    }
}

#[cfg(test)]
mod tests;
