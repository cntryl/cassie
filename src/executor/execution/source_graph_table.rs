//! Execution of a bound `GRAPH_TABLE` source (#1256).
//!
//! Binding is complete; traversal is a separate step. This is the single place a bound
//! `GRAPH_TABLE` source would run.

use super::{QueryError, SourceExecution};
use crate::app::DiagnosticError;

const UNSUPPORTED: &str = "0A000";

pub(super) fn execute() -> SourceExecution {
    Err(QueryError::Cassie(
        DiagnosticError::new(
            UNSUPPORTED,
            "GRAPH_TABLE execution is not available yet".to_owned(),
        )
        .into(),
    ))
}
