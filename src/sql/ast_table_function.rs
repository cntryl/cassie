//! Column shapes of record-returning and `JSON_TABLE` FROM sources (#829).

use serde::{Deserialize, Serialize};

use super::Expr;
use crate::types::DataType;

/// The column list or specification that accompanies a generated table function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TableFunctionForm {
    /// The `AS alias(name type, ...)` column definition list of `json[b]_to_record[set]`.
    Columns(Vec<RecordColumn>),
    /// A `JSON_TABLE(...)` specification. The context item and `PASSING` values are the
    /// function arguments, in that order.
    JsonTable(Box<JsonTableSpec>),
}

/// One declared output column of a column definition list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordColumn {
    pub name: String,
    pub data_type: DataType,
}

/// The top-level `ON ERROR` clause of `JSON_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JsonTableRootError {
    EmptyArray,
    Error,
}

/// A parsed `JSON_TABLE` call without its context item and `PASSING` values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonTableSpec {
    /// The row pattern.
    pub path: String,
    /// The optional `AS name` of the row pattern.
    pub path_name: Option<String>,
    /// `PASSING ... AS name` variable names; their values follow the context item in the
    /// function arguments.
    pub passing: Vec<String>,
    pub columns: Vec<JsonTableColumn>,
    pub on_error: JsonTableRootError,
}

/// One entry of a `COLUMNS` clause.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JsonTableColumn {
    /// `name FOR ORDINALITY`.
    Ordinality { name: String },
    /// `name type [FORMAT JSON] [PATH ...] ...`.
    Value(Box<JsonTableValueColumn>),
    /// `name type EXISTS [PATH ...]`.
    Exists(Box<JsonTableExistsColumn>),
    /// `NESTED [PATH] ... COLUMNS (...)`.
    Nested(Box<JsonTableNested>),
}

/// `WITHOUT`, `WITH CONDITIONAL` or `WITH [UNCONDITIONAL] [ARRAY] WRAPPER`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JsonWrapper {
    Unspecified,
    Without,
    Conditional,
    Unconditional,
}

/// `KEEP QUOTES` or `OMIT QUOTES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JsonQuotes {
    Unspecified,
    Keep,
    Omit,
}

/// An `ON EMPTY` or `ON ERROR` behavior of a value column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JsonBehavior {
    Unspecified,
    Null,
    Error,
    EmptyArray,
    EmptyObject,
    Default(Expr),
}

/// The `ON ERROR` behavior of an `EXISTS` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExistsBehavior {
    Unspecified,
    Error,
    True,
    False,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonTableValueColumn {
    pub name: String,
    pub data_type: DataType,
    pub format_json: bool,
    pub path: String,
    pub wrapper: JsonWrapper,
    pub quotes: JsonQuotes,
    pub on_empty: JsonBehavior,
    pub on_error: JsonBehavior,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonTableExistsColumn {
    pub name: String,
    pub data_type: DataType,
    pub path: String,
    pub on_error: ExistsBehavior,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonTableNested {
    pub path: String,
    pub path_name: Option<String>,
    pub columns: Vec<JsonTableColumn>,
}

impl JsonTableSpec {
    /// Output columns in declaration order, with nested columns inline.
    #[must_use]
    pub fn output_columns(&self) -> Vec<(String, DataType)> {
        let mut columns = Vec::new();
        collect_columns(&self.columns, &mut columns);
        columns
    }
}

fn collect_columns(entries: &[JsonTableColumn], out: &mut Vec<(String, DataType)>) {
    for entry in entries {
        match entry {
            JsonTableColumn::Ordinality { name } => out.push((name.clone(), DataType::Int)),
            JsonTableColumn::Value(column) => {
                out.push((column.name.clone(), column.data_type.clone()));
            }
            JsonTableColumn::Exists(column) => {
                out.push((column.name.clone(), column.data_type.clone()));
            }
            JsonTableColumn::Nested(nested) => collect_columns(&nested.columns, out),
        }
    }
}
