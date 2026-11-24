//! Column shapes of record-returning and `JSON_TABLE` FROM sources (#829).

use serde::{Deserialize, Serialize};

use super::Expr;
use cassie_core::types::DataType;

/// The column list or specification that accompanies a generated table function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TableFunctionForm<A = ()> {
    /// The `AS alias(name type, ...)` column definition list of `json[b]_to_record[set]`.
    Columns(Vec<RecordColumn>),
    /// A `JSON_TABLE(...)` specification. The context item and `PASSING` values are the
    /// function arguments, in that order.
    JsonTable(Box<JsonTableSpec<A>>),
    /// A `GRAPH_TABLE(...)` pattern. Its expressions live in the table itself, so the
    /// call's `function.args` stay empty.
    GraphTable<A>(Box<super::GraphTable<A>>),
    /// The resolved `cassie_graph.graph_element_ref` type of the `vertex` and `edge` columns
    /// of `cassie_graph.path_steps` and `weighted_path_steps`. The binder records it because
    /// the descriptor is per database and the static column list has no catalog (#1256).
    GraphElements(DataType),
    /// The output column types a fixed-shape function resolved while binding, such as the
    /// database's `graph_path` composite for `graph_weighted_paths`.
    Typed(Vec<DataType>),
}

impl<A> TableFunctionForm<A> {
    /// The `GRAPH_TABLE` of this form, if it is one.
    #[must_use]
    pub fn graph_table(&self) -> Option<&super::GraphTable> {
        match self {
            Self::GraphTable(table) => Some(table),
            _ => None,
        }
    }

    /// The `GRAPH_TABLE` of this form, mutably.
    pub fn graph_table_mut(&mut self) -> Option<&mut super::GraphTable> {
        match self {
            Self::GraphTable(table) => Some(table),
            _ => None,
        }
    }

    /// The expressions of a `GRAPH_TABLE` form; empty for every other form.
    pub fn graph_expressions(&self) -> impl Iterator<Item = &Expr> {
        self.graph_table()
            .into_iter()
            .flat_map(super::GraphTable::expressions)
    }

    /// Mutable counterpart of [`TableFunctionForm::graph_expressions`].
    pub fn graph_expressions_mut(&mut self) -> impl Iterator<Item = &mut Expr> {
        match self {
            Self::GraphTable(table) => Some(table),
            _ => None,
        }
        .into_iter()
        .flat_map(|table| table.expressions_mut())
    }
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
pub struct JsonTableSpec<A = ()> {
    /// The row pattern.
    pub path: String,
    /// The optional `AS name` of the row pattern.
    pub path_name: Option<String>,
    /// `PASSING ... AS name` variable names; their values follow the context item in the
    /// function arguments.
    pub passing: Vec<String>,
    pub columns: Vec<JsonTableColumn<A>>,
    pub on_error: JsonTableRootError,
}

/// One entry of a `COLUMNS` clause.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JsonTableColumn<A = ()> {
    /// `name FOR ORDINALITY`.
    Ordinality { name: String },
    /// `name type [FORMAT JSON] [PATH ...] ...`.
    Value(Box<JsonTableValueColumn<A>>),
    /// `name type EXISTS [PATH ...]`.
    Exists(Box<JsonTableExistsColumn>),
    /// `NESTED [PATH] ... COLUMNS (...)`.
    Nested(Box<JsonTableNested<A>>),
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
pub enum JsonBehavior<A = ()> {
    Unspecified,
    Null,
    Error,
    EmptyArray,
    EmptyObject,
    Default(Expr<A>),
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
pub struct JsonTableValueColumn<A = ()> {
    pub name: String,
    pub data_type: DataType,
    pub format_json: bool,
    pub path: String,
    pub wrapper: JsonWrapper,
    pub quotes: JsonQuotes,
    pub on_empty: JsonBehavior<A>,
    pub on_error: JsonBehavior<A>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonTableExistsColumn {
    pub name: String,
    pub data_type: DataType,
    pub path: String,
    pub on_error: ExistsBehavior,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonTableNested<A = ()> {
    pub path: String,
    pub path_name: Option<String>,
    pub columns: Vec<JsonTableColumn<A>>,
}

impl<A> JsonTableSpec<A> {
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
                out.push((column.name.clone(), column.data_type.flattened().clone()));
            }
            JsonTableColumn::Exists(column) => {
                out.push((column.name.clone(), column.data_type.flattened().clone()));
            }
            JsonTableColumn::Nested(nested) => collect_columns(&nested.columns, out),
        }
    }
}
