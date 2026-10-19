use super::{CassieError, CassieSession, QueryStatement};

impl CassieSession {
    pub(crate) fn authorize_statement(
        &self,
        statement: &QueryStatement,
    ) -> Result<(), CassieError> {
        if !self.is_authenticated_read_only() || read_only_statement(statement) {
            return Ok(());
        }

        Err(CassieError::InsufficientPrivilege)
    }
}

fn read_only_statement(statement: &QueryStatement) -> bool {
    match statement {
        // A data-modifying WITH query writes even under a SELECT.
        QueryStatement::Select(select) => !select
            .ctes
            .iter()
            .any(crate::sql::binder::cte_bodies::cte_is_modifying),
        QueryStatement::Show(_)
        | QueryStatement::Set(_)
        | QueryStatement::Transaction(_)
        | QueryStatement::SessionObject(_) => true,
        QueryStatement::Explain(explain) => explain_select(&explain.statement.statement),
        _ => false,
    }
}

fn explain_select(statement: &QueryStatement) -> bool {
    match statement {
        QueryStatement::Select(_) => true,
        QueryStatement::Explain(explain) => explain_select(&explain.statement.statement),
        _ => false,
    }
}
