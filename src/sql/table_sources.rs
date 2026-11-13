//! Selected built-in table-source identities and their physical column shapes.

use crate::app::CassieError;
use crate::sql::ast::{
    Expr, FunctionCall, JsonQuotes, JsonTableValueColumn, JsonWrapper, TableFunctionForm,
};
use crate::types::DataType;

pub(crate) fn is_unnest(name: &str) -> bool {
    name.eq_ignore_ascii_case("unnest") || name.eq_ignore_ascii_case("pg_catalog.unnest")
}
pub(crate) fn is_series(name: &str) -> bool {
    name.eq_ignore_ascii_case("generate_series")
        || name.eq_ignore_ascii_case("pg_catalog.generate_series")
}
pub(crate) fn is_generated(name: &str) -> bool {
    is_unnest(name)
        || is_series(name)
        || json_element_source(name).is_some()
        || record_source(name).is_some()
        || is_json_table(name)
}

pub(crate) fn is_json_table(name: &str) -> bool {
    name.eq_ignore_ascii_case("json_table")
}

/// One of the selected `json[b]_to_record[set]` / `json[b]_populate_record[set]` sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RecordSource {
    /// `jsonb_*` (true) or `json_*` (false).
    pub(crate) binary: bool,
    /// The `_recordset` variants, which expand a JSON array of objects.
    pub(crate) set: bool,
    /// `_populate_*` (true, with a composite base argument) or `_to_*` (false, with a
    /// column definition list).
    pub(crate) populate: bool,
}

impl RecordSource {
    /// Unqualified PostgreSQL function name, used for diagnostics and the default relation alias.
    pub(crate) fn function_name(self) -> String {
        format!(
            "{}_{}_record{}",
            if self.binary { "jsonb" } else { "json" },
            if self.populate { "populate" } else { "to" },
            if self.set { "set" } else { "" }
        )
    }

    /// Position of the JSON document argument.
    pub(crate) const fn document_index(self) -> usize {
        if self.populate {
            1
        } else {
            0
        }
    }

    /// Number of function arguments.
    pub(crate) const fn argument_count(self) -> usize {
        if self.populate {
            2
        } else {
            1
        }
    }
}

pub(crate) fn record_source(name: &str) -> Option<RecordSource> {
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
pub(crate) fn is_query_column(column: &JsonTableValueColumn) -> bool {
    column.format_json
        || column.wrapper != JsonWrapper::Unspecified
        || column.quotes != JsonQuotes::Unspecified
        || matches!(
            column.data_type.flattened(),
            DataType::Json | DataType::Jsonb | DataType::Array(_) | DataType::Composite(_)
        )
}

/// The composite type a `populate_record[set]` base argument is cast to.
pub(crate) fn populate_base_type(function: &FunctionCall) -> Option<crate::types::CompositeRef> {
    match function.args.first()? {
        Expr::Cast {
            data_type: DataType::Composite(descriptor),
            ..
        } => Some(descriptor.clone()),
        _ => None,
    }
}

/// Declared (user-visible) columns of a record-returning or `JSON_TABLE` source.
pub(crate) fn record_declared_columns(
    function: &FunctionCall,
    form: Option<&TableFunctionForm>,
) -> Option<Vec<(String, DataType)>> {
    match form {
        Some(TableFunctionForm::Columns(columns)) => Some(
            columns
                .iter()
                .map(|column| (column.name.clone(), column.data_type.clone()))
                .collect(),
        ),
        Some(TableFunctionForm::JsonTable(spec)) => Some(spec.output_columns()),
        None if record_source(&function.name).is_some_and(|record| record.populate) => Some(
            populate_base_type(function)?
                .fields
                .iter()
                .map(|field| (field.name.clone(), field.data_type.clone()))
                .collect(),
        ),
        None => None,
    }
}

/// Physical output columns of a record-returning or `JSON_TABLE` source, followed by BIGINT
/// ordinality when requested. `None` when the call does not define its shape.
pub(crate) fn record_output_columns(
    function: &FunctionCall,
    with_ordinality: bool,
    form: Option<&TableFunctionForm>,
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
pub(crate) struct JsonElementSource {
    /// `jsonb_*` (true) or `json_*` (false).
    pub(crate) binary: bool,
    /// `json[b]_each[_text]` (true) or `json[b]_array_elements[_text]` (false).
    pub(crate) each: bool,
    /// The `_text` variant, whose value column is TEXT.
    pub(crate) text: bool,
}

impl JsonElementSource {
    /// Unqualified PostgreSQL function name, used for diagnostics and the default relation alias.
    pub(crate) fn function_name(self) -> String {
        format!(
            "{}_{}{}",
            if self.binary { "jsonb" } else { "json" },
            if self.each { "each" } else { "array_elements" },
            if self.text { "_text" } else { "" }
        )
    }

    /// Number of expansion columns, excluding ordinality.
    pub(crate) const fn column_count(self) -> usize {
        if self.each {
            2
        } else {
            1
        }
    }

    /// PostgreSQL's output column name before any column alias list.
    pub(crate) const fn column_name(self, index: usize) -> &'static str {
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

pub(crate) fn json_element_source(name: &str) -> Option<JsonElementSource> {
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
pub(crate) fn json_element_columns(
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

pub(crate) fn physical_name(index: usize) -> String {
    format!("column{}", index + 1)
}

pub(crate) fn unnest_columns(
    function: &FunctionCall,
    with_ordinality: bool,
    mut argument_type: impl FnMut(&Expr) -> Option<DataType>,
) -> Result<Vec<(String, DataType)>, CassieError> {
    let mut columns = Vec::with_capacity(function.args.len() + usize::from(with_ordinality));
    for (index, argument) in function.args.iter().enumerate() {
        let kind = match argument_type(argument) {
            Some(DataType::Array(element)) => scalar_element(*element),
            Some(DataType::Null) | None => DataType::Null,
            Some(_) => {
                return Err(CassieError::Execution(
                    "function unnest(non-array) does not exist".into(),
                ));
            }
        };
        columns.push((physical_name(index), kind));
    }
    if with_ordinality {
        columns.push((physical_name(function.args.len()), DataType::BigInt));
    }
    Ok(columns)
}

fn scalar_element(mut kind: DataType) -> DataType {
    while let DataType::Array(element) = kind {
        kind = *element;
    }
    // An ordinary set-returning FuncExpr has typmod -1 even when the
    // incoming array's elements were explicitly constrained. Values already
    // carry that input coercion; only the scalar result descriptor is erased.
    i32::try_from(kind.type_oid())
        .ok()
        .and_then(super::binder::parameter_data_type_for_oid)
        .unwrap_or(kind)
}
