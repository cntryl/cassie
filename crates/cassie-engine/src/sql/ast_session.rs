//! Session-scoped SQL objects: SQL-level prepared statements, cursors and
//! `DISCARD`.

use serde::{Deserialize, Serialize};

use super::ParsedStatement;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionObjectStatement {
    Discard(DiscardTarget),
    Prepare(PrepareStatement),
    Execute(ExecuteStatement),
    /// `DEALLOCATE [PREPARE] name`; `None` is `DEALLOCATE ALL`.
    Deallocate(Option<String>),
    DeclareCursor(DeclareCursorStatement),
    Fetch(FetchStatement),
    /// `CLOSE name`; `None` is `CLOSE ALL`.
    CloseCursor(Option<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiscardTarget {
    All,
    Plans,
    Sequences,
    Temp,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareStatement {
    pub name: String,
    /// Declared parameter types, as written.
    pub parameter_types: Vec<String>,
    /// The prepared statement's SQL text.
    pub statement_sql: String,
    pub statement: Box<ParsedStatement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteStatement {
    pub name: String,
    /// Argument expressions, as written.
    pub arguments: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeclareCursorStatement {
    pub name: String,
    pub binary: bool,
    /// `SCROLL` is `Some(true)`, `NO SCROLL` is `Some(false)`.
    pub scroll: Option<bool>,
    pub hold: bool,
    pub query_sql: String,
    pub query: Box<ParsedStatement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchStatement {
    pub cursor: String,
    pub direction: FetchDirection,
    /// `MOVE` positions the cursor without returning rows.
    pub move_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FetchDirection {
    /// `NEXT`, `count`, `FORWARD [count]`; `None` is `ALL`.
    Forward(Option<i64>),
    /// `PRIOR`, `BACKWARD [count]`; `None` is `ALL`.
    Backward(Option<i64>),
    /// `FIRST`, `LAST`, `ABSOLUTE count`.
    Absolute(i64),
    /// `RELATIVE count`.
    Relative(i64),
}

impl SessionObjectStatement {
    /// The statement's command tag, before any row count.
    #[must_use]
    pub const fn command_name(&self) -> &'static str {
        match self {
            Self::Discard(DiscardTarget::All) => "DISCARD ALL",
            Self::Discard(DiscardTarget::Plans) => "DISCARD PLANS",
            Self::Discard(DiscardTarget::Sequences) => "DISCARD SEQUENCES",
            Self::Discard(DiscardTarget::Temp) => "DISCARD TEMP",
            Self::Prepare(_) => "PREPARE",
            Self::Execute(_) => "EXECUTE",
            Self::Deallocate(Some(_)) => "DEALLOCATE",
            Self::Deallocate(None) => "DEALLOCATE ALL",
            Self::DeclareCursor(_) => "DECLARE CURSOR",
            Self::Fetch(FetchStatement {
                move_only: true, ..
            }) => "MOVE",
            Self::Fetch(_) => "FETCH",
            Self::CloseCursor(Some(_)) => "CLOSE CURSOR",
            Self::CloseCursor(None) => "CLOSE CURSOR ALL",
        }
    }
}
