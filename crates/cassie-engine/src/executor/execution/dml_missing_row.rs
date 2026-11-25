//! Error for a row that a statement matched but is no longer current.
use super::{CassieSession, QueryError};
use crate::app::{CassieError, TransactionErrorKind};

/// A row matched through the statement view that is absent from current
/// storage was removed by another committer. Under a retained REPEATABLE READ
/// owner that is a serialization failure; otherwise it stays an execution error.
pub(super) fn missing_row_error(
    session: Option<&CassieSession>,
    row_id: &str,
    table: &str,
) -> QueryError {
    if session.is_some_and(CassieSession::is_repeatable_read_write) {
        return QueryError::from(CassieError::Transaction {
            kind: TransactionErrorKind::SerializationFailure,
            message: format!(
                "could not serialize access due to concurrent delete of row '{row_id}' in '{table}'"
            ),
        });
    }
    QueryError::General(format!("row '{row_id}' was not found in '{table}'"))
}
