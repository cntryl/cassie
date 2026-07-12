use crate::sql::ast::{QueryStatement, StatementFamily};

use super::{CassieError, CassieSession};

pub(crate) fn ensure_supported_transaction_semantics(
    session: &CassieSession,
    statement: &QueryStatement,
) -> Result<(), CassieError> {
    if !session.is_transaction_active() {
        return Ok(());
    }

    if matches!(statement, QueryStatement::Copy(_)) {
        return reject_active_transaction_statement(
            session,
            "COPY is not supported inside an active transaction",
        );
    }

    if statement.family() != StatementFamily::Runtime {
        return reject_active_transaction_statement(
            session,
            "DDL and catalog operations are not supported inside an active transaction",
        );
    }

    Ok(())
}

fn reject_active_transaction_statement(
    session: &CassieSession,
    message: &str,
) -> Result<(), CassieError> {
    session.mark_transaction_failed();
    Err(CassieError::Unsupported(message.to_string()))
}
