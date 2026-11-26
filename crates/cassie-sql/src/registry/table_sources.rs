//! Portable identities and declared shapes of selected SQL table sources.

use crate::ast::{
    Expr, FunctionCall, GraphTable, JsonQuotes, JsonTableValueColumn, JsonWrapper,
    TableFunctionForm,
};
use cassie_core::types::{CompositeRef, DataType};

pub const GRAPH_TABLE_FUNCTION: &str = "graph_table";

#[must_use]
pub fn is_unnest(name: &str) -> bool {
    name.eq_ignore_ascii_case("unnest") || name.eq_ignore_ascii_case("pg_catalog.unnest")
}

#[must_use]
pub fn is_series(name: &str) -> bool {
    name.eq_ignore_ascii_case("generate_series")
        || name.eq_ignore_ascii_case("pg_catalog.generate_series")
}

#[must_use]
pub fn is_partition_ancestors(name: &str) -> bool {
    name.eq_ignore_ascii_case("pg_partition_ancestors")
        || name.eq_ignore_ascii_case("pg_catalog.pg_partition_ancestors")
}

#[must_use]
pub fn is_generated(name: &str) -> bool {
    is_unnest(name)
        || is_series(name)
        || is_partition_ancestors(name)
        || json_element_source(name).is_some()
        || record_source(name).is_some()
        || is_json_table(name)
        || is_graph_table(name)
}

#[must_use]
pub fn is_json_table(name: &str) -> bool {
    name.eq_ignore_ascii_case("json_table")
}

/// One of the selected `json[b]_to_record[set]` / `json[b]_populate_record[set]` sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordSource {
    /// `jsonb_*` (true) or `json_*` (false).
    pub binary: bool,
    /// The `_recordset` variants, which expand a JSON array of objects.
    pub set: bool,
    /// `_populate_*` (true, with a composite base argument) or `_to_*` (false, with a
    /// column definition list).
    pub populate: bool,
}

impl RecordSource {
    /// Unqualified PostgreSQL function name, used for diagnostics and the default relation alias.
    #[must_use]
    pub fn function_name(self) -> String {
        format!(
            "{}_{}_record{}",
            if self.binary { "jsonb" } else { "json" },
            if self.populate { "populate" } else { "to" },
            if self.set { "set" } else { "" }
        )
    }

    /// Position of the JSON document argument.
    #[must_use]
    pub const fn document_index(self) -> usize {
        if self.populate {
            1
        } else {
            0
        }
    }

    /// Number of function arguments.
    #[must_use]
    pub const fn argument_count(self) -> usize {
        if self.populate {
            2
        } else {
            1
        }
    }
}

#[must_use]
pub fn record_source(name: &str) -> Option<RecordSource> {
    let lower = name.to_ascii_lowercase();
    let bare = lower.strip_prefix("pg_catalog.").unwrap_or(&lower);
    let (binary, rest) = if let Some(rest) = bare.strip_prefix("jsonb_") {
        (true, rest)
    } else {
        (false, bare.strip_prefix("json_")?)
    };
    let (populate, set) = match rest {
        "to_record" => (false, false),
        "to_recordset" => (false, true),
        "populate_record" => (true, false),
        "populate_recordset" => (true, true),
        _ => return None,
    };
    Some(RecordSource {
        binary,
        set,
        populate,
    })
}

/// Whether a regular `JSON_TABLE` column follows `JSON_QUERY` (rather than `JSON_VALUE`)
/// semantics: it asks for JSON format, a wrapper or quote handling, or has a JSON, array or
/// composite type.
#[must_use]
pub fn is_query_column<A>(column: &JsonTableValueColumn<A>) -> bool {
    column.format_json
        || column.wrapper != JsonWrapper::Unspecified
        || column.quotes != JsonQuotes::Unspecified
        || matches!(
            column.data_type.flattened(),
            DataType::Json | DataType::Jsonb | DataType::Array(_) | DataType::Composite(_)
        )
}

/// The composite type a `populate_record[set]` base argument is cast to.
#[must_use]
pub fn populate_base_type<A>(function: &FunctionCall<A>) -> Option<CompositeRef> {
    match function.args.first()? {
        Expr::Cast {
            data_type: DataType::Composite(descriptor),
            ..
        } => Some(descriptor.clone()),
        _ => None,
    }
}

/// Declared (user-visible) columns of a record-returning or `JSON_TABLE` source.
#[must_use]
pub fn record_declared_columns<A>(
    function: &FunctionCall<A>,
    form: Option<&TableFunctionForm<A>>,
) -> Option<Vec<(String, DataType)>> {
    match form {
        Some(TableFunctionForm::Columns(columns)) => Some(
            columns
                .iter()
                .map(|column| (column.name.clone(), column.data_type.clone()))
                .collect(),
        ),
        Some(TableFunctionForm::JsonTable(spec)) => Some(spec.output_columns()),
        Some(TableFunctionForm::GraphTable(table)) => Some(graph_table_columns(table)),
        None if record_source(&function.name).is_some_and(|record| record.populate) => Some(
            populate_base_type(function)?
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        ),
        Some(TableFunctionForm::GraphElements(_) | TableFunctionForm::Typed(_)) | None => None,
    }
}

/// Declared columns of a `GRAPH_TABLE` source: projection names, typed once bound and `NULL`-typed
/// before.
fn graph_table_columns<A>(table: &GraphTable<A>) -> Vec<(String, DataType)> {
    table
        .columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let name = column.output_name().unwrap_or_default();
            let data_type = table
                .binding
                .as_ref()
                .and_then(|binding| binding.output.get(index))
                .cloned()
                .unwrap_or(DataType::Null);
            (name, data_type)
        })
        .collect()
}

#[must_use]
pub fn is_graph_table(name: &str) -> bool {
    name.eq_ignore_ascii_case(GRAPH_TABLE_FUNCTION)
}

/// Physical output columns of a record-returning or `JSON_TABLE` source, followed by BIGINT
/// ordinality when requested. `None` when the call does not define its shape.
#[must_use]
pub fn record_output_columns<A>(
    function: &FunctionCall<A>,
    with_ordinality: bool,
    form: Option<&TableFunctionForm<A>>,
) -> Option<Vec<(String, DataType)>> {
    let mut columns = record_declared_columns(function, form)?
        .into_iter()
        .enumerate()
        // Output columns are typed by the base type; the domain only constrains values.
        .map(|(index, (_, data_type))| (physical_name(index), data_type.flattened().clone()))
        .collect::<Vec<_>>();
    if with_ordinality {
        columns.push((physical_name(columns.len()), DataType::BigInt));
    }
    Some(columns)
}

/// One of the selected `json[b]_array_elements[_text]` / `json[b]_each[_text]` sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsonElementSource {
    /// `jsonb_*` (true) or `json_*` (false).
    pub binary: bool,
    /// `json[b]_each[_text]` (true) or `json[b]_array_elements[_text]` (false).
    pub each: bool,
    /// The `_text` variant, whose value column is TEXT.
    pub text: bool,
}

impl JsonElementSource {
    /// Unqualified PostgreSQL function name, used for diagnostics and the default relation alias.
    #[must_use]
    pub fn function_name(self) -> String {
        format!(
            "{}_{}{}",
            if self.binary { "jsonb" } else { "json" },
            if self.each { "each" } else { "array_elements" },
            if self.text { "_text" } else { "" }
        )
    }

    /// Number of expansion columns, excluding ordinality.
    #[must_use]
    pub const fn column_count(self) -> usize {
        if self.each {
            2
        } else {
            1
        }
    }

    /// PostgreSQL's output column name before any column alias list.
    #[must_use]
    pub const fn column_name(self, index: usize) -> &'static str {
        if self.each && index == 0 {
            "key"
        } else {
            "value"
        }
    }

    fn value_type(self) -> DataType {
        if self.text {
            DataType::Text
        } else if self.binary {
            DataType::Jsonb
        } else {
            DataType::Json
        }
    }
}

#[must_use]
pub fn json_element_source(name: &str) -> Option<JsonElementSource> {
    let lower = name.to_ascii_lowercase();
    let bare = lower.strip_prefix("pg_catalog.").unwrap_or(&lower);
    let (binary, rest) = if let Some(rest) = bare.strip_prefix("jsonb_") {
        (true, rest)
    } else {
        (false, bare.strip_prefix("json_")?)
    };
    let (each, text) = match rest {
        "array_elements" => (false, false),
        "array_elements_text" => (false, true),
        "each" => (true, false),
        "each_text" => (true, true),
        _ => return None,
    };
    Some(JsonElementSource { binary, each, text })
}

/// Physical columns of a JSON expansion source, followed by BIGINT ordinality when requested.
#[must_use]
pub fn json_element_columns(
    source: JsonElementSource,
    with_ordinality: bool,
) -> Vec<(String, DataType)> {
    let mut columns = Vec::with_capacity(source.column_count() + 1);
    if source.each {
        columns.push((physical_name(0), DataType::Text));
    }
    columns.push((physical_name(columns.len()), source.value_type()));
    if with_ordinality {
        columns.push((physical_name(columns.len()), DataType::BigInt));
    }
    columns
}

/// The JSON object key a declared column reads, preserving the identifier text rules.
#[must_use]
pub fn json_member_name(declared: &str) -> String {
    declared
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .map_or_else(|| declared.to_owned(), |inner| inner.replace("\"\"", "\""))
}

#[must_use]
pub fn physical_name(index: usize) -> String {
    format!("column{}", index + 1)
}

#[cfg(test)]
mod tests {
    use super::{is_generated, json_element_source, json_member_name, record_source};

    #[test]
    fn should_classify_selected_table_sources_without_engine_state() {
        // Arrange
        let generated = [
            "unnest",
            "pg_catalog.generate_series",
            "jsonb_each_text",
            "json_to_recordset",
            "json_table",
            "graph_table",
        ];

        // Act
        let classifications = generated.map(is_generated);

        // Assert
        assert!(classifications.into_iter().all(|classified| classified));
        assert!(
            json_element_source("pg_catalog.JSONB_EACH_TEXT")
                .unwrap()
                .binary
        );
        assert!(
            record_source("pg_catalog.JSONB_POPULATE_RECORDSET")
                .unwrap()
                .populate
        );
        assert_eq!(json_member_name("\"user\"\"Id\""), "user\"Id");
    }
}
