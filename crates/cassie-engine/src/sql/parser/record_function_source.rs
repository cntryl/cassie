//! Column definition lists of `json[b]_to_record[set]` and aliases of
//! `json[b]_populate_record[set]` FROM sources.

use super::super::domain_scope::parse_declared_data_type;
use super::super::identifiers::normalize_column_identifier;
use super::super::lexical;
use super::json_table_source::alias_with_declared_columns;
use super::{split_csv, QuerySource, SqlError};
use crate::sql::ast::{FunctionCall, RecordColumn, TableFunctionForm};
use crate::sql::table_sources::{self, RecordSource};

const REQUIRED: &str = "a column definition list is required for functions returning \"record\"";
const REDUNDANT: &str =
    "a column definition list is redundant for a function returning a named composite type";
const ORDINALITY: &str = "WITH ORDINALITY cannot be used with a column definition list";

pub(super) fn parse(
    function: FunctionCall,
    record: RecordSource,
    tail: &str,
    with_ordinality: bool,
) -> Result<QuerySource, SqlError> {
    let (head, definitions) = split_definitions(tail)?;
    if record.populate {
        parse_populate(function, definitions, tail, with_ordinality)
    } else {
        parse_to_record(function, head, definitions, with_ordinality)
    }
}

/// Splits `[AS] alias ( definitions )` into the alias text and the parenthesized list.
fn split_definitions(tail: &str) -> Result<(&str, Option<&str>), SqlError> {
    let tail = lexical::trim_separators(tail);
    let Some((open, _)) = lexical::find_top_level_span(tail, 0, "(") else {
        return Ok((tail, None));
    };
    let close = lexical::matching_paren(tail, open)
        .ok_or_else(|| SqlError::syntax("invalid column definition list".into()))?;
    if !lexical::trim_separators(&tail[close + 1..]).is_empty() {
        return Err(SqlError::new("unsupported FROM syntax".into()));
    }
    Ok((&tail[..open], Some(&tail[open + 1..close])))
}

fn parse_to_record(
    function: FunctionCall,
    head: &str,
    definitions: Option<&str>,
    with_ordinality: bool,
) -> Result<QuerySource, SqlError> {
    let Some(definitions) = definitions else {
        return Err(SqlError::syntax(REQUIRED.into()));
    };
    if with_ordinality {
        return Err(SqlError::syntax(ORDINALITY.into()));
    }
    let columns = split_csv(definitions)
        .into_iter()
        .map(parse_definition)
        .collect::<Result<Vec<_>, _>>()?;
    for (index, column) in columns.iter().enumerate() {
        if columns[..index]
            .iter()
            .any(|prior| prior.name == column.name)
        {
            return Err(SqlError::duplicate_column(&column.name));
        }
    }
    let names = columns
        .iter()
        .map(|column| column.name.clone())
        .collect::<Vec<_>>();
    let source = QuerySource::TableFunction {
        name: function.name.clone(),
        function,
        lateral: true,
        with_ordinality: false,
        form: Some(Box::new(TableFunctionForm::Columns(columns))),
    };
    alias_with_declared_columns(source, head, &names)
}

/// `name type`, where the type may contain spaces, a modifier and an array suffix.
fn parse_definition(definition: &str) -> Result<RecordColumn, SqlError> {
    let definition = lexical::trim_separators(definition);
    let name_end = if definition.starts_with('"') {
        lexical::quoted_end(definition, 0)
    } else {
        definition.find(char::is_whitespace)
    }
    .ok_or_else(|| SqlError::syntax(REQUIRED.into()))?;
    let type_text = lexical::trim_separators(&definition[name_end..]);
    if type_text.is_empty() {
        return Err(SqlError::syntax(REQUIRED.into()));
    }
    Ok(RecordColumn {
        name: normalize_column_identifier(&definition[..name_end])?,
        data_type: parse_declared_data_type(type_text)?,
    })
}

fn parse_populate(
    function: FunctionCall,
    definitions: Option<&str>,
    tail: &str,
    with_ordinality: bool,
) -> Result<QuerySource, SqlError> {
    // An alias list names columns; a typed list would be a column definition list.
    if definitions.is_some_and(|list| split_csv(list).into_iter().any(is_typed_entry)) {
        return Err(SqlError::syntax(REDUNDANT.into()));
    }
    let declared =
        table_sources::record_declared_columns(&function, None).map_or_else(Vec::new, |columns| {
            columns
                .into_iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>()
        });
    let source = QuerySource::TableFunction {
        name: function.name.clone(),
        function,
        lateral: true,
        with_ordinality,
        form: None,
    };
    alias_with_declared_columns(source, tail, &declared)
}

/// Whether an alias-list entry carries a type after its name.
fn is_typed_entry(entry: &str) -> bool {
    let entry = lexical::trim_separators(entry);
    if entry.starts_with('"') {
        lexical::quoted_end(entry, 0)
            .is_some_and(|end| !lexical::trim_separators(&entry[end..]).is_empty())
    } else {
        entry.contains(char::is_whitespace)
    }
}
