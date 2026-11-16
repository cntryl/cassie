//! Hook of the shared `cassie_graph` validator into record construction (#1268).
use crate::app::{CassieError, DiagnosticError};
use crate::executor::QueryError;
use crate::types::RecordValue;

/// Validates `record` when it is a reserved graph type (SQLSTATE `22023`);
/// every other record passes untouched.
///
/// # Errors
/// Returns `22023` with the violated invariant.
pub(crate) fn validate(record: &RecordValue) -> Result<(), QueryError> {
    crate::types::graph::validate_record(record).map_err(|error| {
        QueryError::Cassie(CassieError::from(DiagnosticError::new(
            "22023",
            error.message().to_owned(),
        )))
    })
}
