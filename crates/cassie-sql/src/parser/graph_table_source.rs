//! `GRAPH_TABLE (...)` as a FROM source (#1256).

use super::super::graph_table::parse_graph_table;
use super::super::{lexical, SqlError};
use super::json_table_source::alias_with_declared_columns;
use super::QuerySource;
use crate::ast::{FunctionCall, GraphTable, TableFunctionForm};

use crate::registry::table_sources::GRAPH_TABLE_FUNCTION as FUNCTION_NAME;

/// Parses a `GRAPH_TABLE (...)` source with its optional `[AS] alias [(columns)]`, or `None`
/// when `raw` is another kind of source.
pub(super) fn parse(raw: &str, lateral: bool) -> Result<Option<QuerySource>, SqlError> {
    let text = lexical::trim_separators(raw);
    let Some(after_keyword) = lexical::pattern_end(text, 0, FUNCTION_NAME) else {
        return Ok(None);
    };
    if !lexical::trim_separators(&text[after_keyword..]).starts_with('(') {
        return Ok(None);
    }
    let (table, consumed) = parse_graph_table(text)?;
    let declared = output_names(&table)?;
    let source = QuerySource::TableFunction {
        name: FUNCTION_NAME.to_owned(),
        function: FunctionCall {
            name: FUNCTION_NAME.to_owned(),
            ..FunctionCall::default()
        },
        lateral,
        with_ordinality: false,
        form: Some(Box::new(TableFunctionForm::GraphTable(Box::new(table)))),
    };
    alias_with_declared_columns(source, &text[consumed..], &declared).map(Some)
}

/// The name of each `COLUMNS` projection.
fn output_names(table: &GraphTable) -> Result<Vec<String>, SqlError> {
    table
        .columns
        .iter()
        .map(|column| {
            column.output_name().ok_or_else(|| {
                SqlError::syntax("GRAPH_TABLE COLUMNS expression requires an AS alias".into())
            })
        })
        .collect()
}

#[cfg(test)]
#[path = "graph_table_source_tests.rs"]
mod tests;
