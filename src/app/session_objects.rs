//! Execution of SQL-level prepared statements, cursors and `DISCARD`.

#[path = "session_objects/cursors.rs"]
mod cursors;
#[path = "session_objects/discard.rs"]
mod discard;
#[path = "session_objects/fetch_plan.rs"]
mod fetch_plan;
#[path = "session_objects/prepared.rs"]
mod prepared;
pub(crate) use prepared::parameter_type_display;

use super::{Cassie, CassieError, CassieSession, QueryExecutionControls, QueryResult};
use crate::runtime::ExecutionMode;
use crate::sql::ast::SessionObjectStatement;

/// A prepared statement, cursor or session-reset failure with its
/// PostgreSQL SQLSTATE.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct SessionObjectError {
    pub(crate) sqlstate: &'static str,
    pub(crate) message: String,
}

impl SessionObjectError {
    fn new(sqlstate: &'static str, message: String) -> Self {
        Self { sqlstate, message }
    }

    pub(crate) fn missing_prepared(name: &str) -> Self {
        Self::new(
            "26000",
            format!("prepared statement \"{name}\" does not exist"),
        )
    }

    pub(crate) fn duplicate_prepared(name: &str) -> Self {
        Self::new(
            "42P05",
            format!("prepared statement \"{name}\" already exists"),
        )
    }

    fn missing_cursor(name: &str) -> Self {
        Self::new("34000", format!("cursor \"{name}\" does not exist"))
    }

    fn duplicate_cursor(name: &str) -> Self {
        Self::new("42P03", format!("cursor \"{name}\" already exists"))
    }

    fn scan_forward_only() -> Self {
        Self::new("55000", "cursor can only scan forward".to_string())
    }

    fn wrong_parameter_count(name: &str) -> Self {
        Self::new(
            "42601",
            format!("wrong number of parameters for prepared statement \"{name}\""),
        )
    }
}

/// How a session-object statement was submitted.
#[derive(Clone, Copy)]
pub(super) struct SessionObjectCall<'a> {
    pub(super) controls: &'a QueryExecutionControls,
    pub(super) mode: ExecutionMode,
    /// Whether results are described for the wire protocol.
    pub(super) wire_output: bool,
}

impl Cassie {
    pub(super) fn execute_session_object(
        &self,
        session: &CassieSession,
        statement: &SessionObjectStatement,
        raw_sql: &str,
        call: SessionObjectCall<'_>,
    ) -> Result<QueryResult, CassieError> {
        match statement {
            SessionObjectStatement::Discard(target) => {
                Self::execute_discard(session, *target, call.controls)
            }
            SessionObjectStatement::Prepare(prepare) => {
                self.execute_prepare(session, prepare, raw_sql, call)
            }
            SessionObjectStatement::Execute(execute) => {
                self.execute_prepared_sql(session, execute, call)
            }
            SessionObjectStatement::Deallocate(name) => {
                Self::execute_deallocate(session, name.as_deref(), call.controls)
            }
            SessionObjectStatement::DeclareCursor(declare) => {
                self.execute_declare(session, declare, raw_sql, call)
            }
            SessionObjectStatement::Fetch(fetch) => {
                Self::execute_fetch(session, fetch, call.controls)
            }
            SessionObjectStatement::CloseCursor(name) => {
                Self::execute_close(session, name.as_deref(), call.controls)
            }
        }
    }

    /// The result columns a session-object statement returns.
    pub(super) fn describe_session_object(
        &self,
        session: Option<&CassieSession>,
        statement: &SessionObjectStatement,
        wire_output: bool,
    ) -> Result<Vec<crate::executor::ColumnMeta>, CassieError> {
        let Some(session) = session else {
            return Ok(Vec::new());
        };
        match statement {
            SessionObjectStatement::Execute(execute) => {
                self.describe_prepared_sql(session, execute, wire_output)
            }
            SessionObjectStatement::Fetch(fetch) if !fetch.move_only => {
                Self::describe_fetch(session, fetch)
            }
            _ => Ok(Vec::new()),
        }
    }
}

/// A command-only result carrying `command` as its tag.
fn command_result(
    command: &str,
    controls: &QueryExecutionControls,
) -> Result<QueryResult, CassieError> {
    let mut result = QueryResult::try_builder(controls)?;
    result.try_set_command(command)?;
    result.finish()
}
