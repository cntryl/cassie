//! Declared ARRAY ordering without changing JSON values or storage encoding.
use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::ops::ControlFlow;

use crate::catalog::FunctionMeta;
use crate::sql::ast::{CteSearchOrder, Expr};
use crate::types::{DataType, Schema, Value};

use super::batch::RowAccess;
use super::semantic::{compare_array_parts, SemanticValue};

const CTE_LINEAGE_COLUMN_PREFIX: &str = "\0cassie_cte_lineage:";

#[derive(serde::Deserialize)]
struct SearchLineage {
    search_path: Vec<Vec<Value>>,
    #[serde(default)]
    #[serde(rename = "search_column")]
    column: Option<String>,
    #[serde(default)]
    #[serde(rename = "search_order")]
    order: Option<CteSearchOrder>,
}

pub(super) fn expression_type<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    functions: &HashMap<String, FunctionMeta>,
) -> Option<DataType> {
    if let Expr::Column(name) = expr {
        return row.column_type(name).cloned();
    }
    let mut columns = Vec::new();
    collect_columns(row, expr, &mut columns);
    crate::sql::binder::infer_expr_type(expr, &Schema::from_iter(columns), functions, &[])
}

fn collect_columns<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    columns: &mut Vec<(String, DataType)>,
) {
    if let Expr::Column(name) = expr {
        if let Some(data_type) = row.column_type(name) {
            columns.push((name.clone(), data_type.clone()));
        }
    }
    let _: ControlFlow<()> = expr.try_for_each_child(|child| {
        collect_columns(row, child, columns);
        ControlFlow::Continue(())
    });
}

pub(super) fn key<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    value: &Value,
    functions: &HashMap<String, FunctionMeta>,
) -> SemanticValue {
    if let Some(key) = search_order_key(row, expr) {
        return key;
    }
    let declared = match expr {
        Expr::Column(name) => row.column_type(name),
        Expr::Cast { data_type, .. } => Some(data_type),
        _ => None,
    };
    if matches!(declared, Some(DataType::Char { .. })) {
        return super::semantic::typed_value(value, declared);
    }
    if !row.has_array_types() {
        return SemanticValue::from_value(value);
    }
    typed_key(value, expression_type(row, expr, functions).as_ref())
}

pub(super) fn search_order_metadata_bytes<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
) -> Option<usize> {
    searched_column(expr)?;
    let bytes = row
        .entries()
        .iter()
        .filter(|(name, _)| name.starts_with(CTE_LINEAGE_COLUMN_PREFIX))
        .filter_map(|(_, value)| match value {
            Value::String(json) => Some(json.len()),
            _ => None,
        })
        .fold(0usize, usize::saturating_add);
    (bytes > 0).then_some(bytes)
}

fn search_order_key<R: RowAccess + ?Sized>(row: &R, expr: &Expr) -> Option<SemanticValue> {
    let column = searched_column(expr)?;
    row.entries()
        .iter()
        .filter(|(name, _)| name.starts_with(CTE_LINEAGE_COLUMN_PREFIX))
        .find_map(|(_, value)| {
            let Value::String(json) = value else {
                return None;
            };
            let lineage = serde_json::from_str::<SearchLineage>(json).ok()?;
            if lineage.column.as_deref() != Some(column.as_str()) {
                return None;
            }
            match lineage.order? {
                CteSearchOrder::DepthFirst => Some(SemanticValue::Array(
                    lineage
                        .search_path
                        .into_iter()
                        .map(|record| {
                            SemanticValue::Array(
                                record.iter().map(SemanticValue::from_value).collect(),
                            )
                        })
                        .collect(),
                )),
                CteSearchOrder::BreadthFirst => {
                    let keys = lineage.search_path.last()?;
                    let depth = i64::try_from(lineage.search_path.len().saturating_sub(1))
                        .unwrap_or(i64::MAX);
                    let mut fields = Vec::with_capacity(keys.len() + 1);
                    fields.push(SemanticValue::from_value(&Value::Int64(depth)));
                    fields.extend(keys.iter().map(SemanticValue::from_value));
                    Some(SemanticValue::Array(fields))
                }
            }
        })
}

fn searched_column(expr: &Expr) -> Option<String> {
    let Expr::Column(name) = expr else {
        return None;
    };
    crate::sql::ColumnIdentifierPath::parse(name)
        .ok()
        .map(|column| column.final_name().to_owned())
}

fn typed_key(value: &Value, data_type: Option<&DataType>) -> SemanticValue {
    let Some(DataType::Array(element_type)) = data_type else {
        return SemanticValue::from_value(value);
    };
    match value {
        Value::Json(json) => {
            array_key(json, element_type).unwrap_or_else(|| SemanticValue::from_value(value))
        }
        Value::String(text) => serde_json::from_str(text)
            .ok()
            .and_then(|json| array_key(&json, element_type))
            .unwrap_or_else(|| SemanticValue::from_value(value)),
        _ => SemanticValue::from_value(value),
    }
}

fn array_key(json: &serde_json::Value, element_type: &DataType) -> Option<SemanticValue> {
    Some(SemanticValue::Array(
        json.as_array()?
            .iter()
            .map(|value| element_key(value, element_type))
            .collect(),
    ))
}

fn element_key(value: &serde_json::Value, data_type: &DataType) -> SemanticValue {
    if matches!(data_type, DataType::Json) && !value.is_null() {
        return SemanticValue::from_value(&Value::Json(value.clone()));
    }
    match value {
        serde_json::Value::Null => SemanticValue::Null,
        serde_json::Value::Bool(value) => SemanticValue::from_value(&Value::Bool(*value)),
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                SemanticValue::from_value(&Value::Int64(value))
            } else if let Some(value) = number.as_f64() {
                SemanticValue::from_value(&Value::Float64(value))
            } else {
                SemanticValue::from_value(&Value::Json(value.clone()))
            }
        }
        serde_json::Value::String(text) => {
            let canonical =
                match data_type {
                    DataType::Char { .. } => {
                        crate::types::char_text::canonical_char_text(text).to_owned()
                    }
                    DataType::Date => crate::types::temporal::canonical_date(text)
                        .unwrap_or_else(|_| text.clone()),
                    DataType::Time => crate::types::temporal::canonical_time(text)
                        .unwrap_or_else(|_| text.clone()),
                    DataType::Timestamp => crate::types::temporal::canonical_timestamp(text)
                        .unwrap_or_else(|_| text.clone()),
                    DataType::Uuid => uuid::Uuid::parse_str(text)
                        .map_or_else(|_| text.clone(), |value| value.to_string()),
                    DataType::Bytea => crate::midge::row_blob::canonical_bytea_text(text)
                        .unwrap_or_else(|_| text.clone()),
                    _ => text.clone(),
                };
            SemanticValue::from_value(&Value::String(canonical))
        }
        serde_json::Value::Array(values) if matches!(data_type, DataType::Vector(_)) => {
            SemanticValue::Array(
                values
                    .iter()
                    .map(|value| element_key(value, &DataType::Float))
                    .collect(),
            )
        }
        _ => SemanticValue::from_value(&Value::Json(value.clone())),
    }
}

/// EXCLUDED values are local arguments from the same target schema as the
/// existing conflict row. Resolve their declared type without copying their
/// values into that row or changing normal value lookup.
struct ComparisonScope<'a, R: ?Sized> {
    row: &'a R,
    local_args: Option<&'a HashMap<String, Value>>,
}

impl<R: RowAccess + ?Sized> RowAccess for ComparisonScope<'_, R> {
    fn get(&self, name: &str) -> Option<&Value> {
        self.row.get(name)
    }
    fn entries(&self) -> &[(String, Value)] {
        self.row.entries()
    }
    fn has_array_types(&self) -> bool {
        self.row.has_array_types()
    }
    fn column_type(&self, name: &str) -> Option<&DataType> {
        self.row.column_type(name).or_else(|| {
            let key = crate::sql::ColumnIdentifierPath::parse(name)
                .map_or_else(|_| name.to_ascii_lowercase(), |column| column.lookup_key());
            let local_args = self.local_args?;
            if !local_args.contains_key(&key) {
                return None;
            }
            self.row.column_type(key.strip_prefix("excluded.")?)
        })
    }
}

/// Only untyped parameters inherit an ARRAY peer's element type. A declared
/// JSON column never becomes an ARRAY merely because its peer is one.
pub(super) fn compare<R: RowAccess + ?Sized>(
    row: &R,
    left: (&Expr, &Value),
    right: (&Expr, &Value),
    functions: &HashMap<String, FunctionMeta>,
    local_args: Option<&HashMap<String, Value>>,
) -> Option<Ordering> {
    if !row.has_array_types() {
        return None;
    }
    if matches!(left.1, Value::Null) || matches!(right.1, Value::Null) {
        return None;
    }
    let scope = ComparisonScope { row, local_args };
    let mut left_type = expression_type(&scope, left.0, functions);
    let mut right_type = expression_type(&scope, right.0, functions);
    if matches!(left_type, Some(DataType::Array(_))) && parameter_context(right.0) {
        right_type.clone_from(&left_type);
    }
    if matches!(right_type, Some(DataType::Array(_))) && parameter_context(left.0) {
        left_type.clone_from(&right_type);
    }
    if !matches!(left_type, Some(DataType::Array(_)))
        || !matches!(right_type, Some(DataType::Array(_)))
    {
        return None;
    }
    let (Some(DataType::Array(left_type)), Some(DataType::Array(right_type))) =
        (left_type, right_type)
    else {
        return None;
    };
    let left = array_value(left.1)?;
    let right = array_value(right.1)?;
    Some(compare_arrays(
        left.as_array()?,
        &left_type,
        right.as_array()?,
        &right_type,
    ))
}

fn array_value(value: &Value) -> Option<Cow<'_, serde_json::Value>> {
    match value {
        Value::Json(value) => Some(Cow::Borrowed(value)),
        Value::String(text) => serde_json::from_str(text).ok().map(Cow::Owned),
        _ => None,
    }
}

// Predicates compare borrowed elements rather than retaining a key for every
// element. Sort and aggregate keys use the same element and prefix semantics.
fn compare_arrays(
    left: &[serde_json::Value],
    left_type: &DataType,
    right: &[serde_json::Value],
    right_type: &DataType,
) -> Ordering {
    compare_array_parts(left, right, |left, right| {
        match (left.is_null(), right.is_null()) {
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (true, true) => Ordering::Equal,
            (false, false) => {
                if matches!(
                    (left_type, right_type),
                    (DataType::Vector(_), DataType::Vector(_))
                ) {
                    if let (Some(left), Some(right)) = (left.as_array(), right.as_array()) {
                        return compare_arrays(left, &DataType::Float, right, &DataType::Float);
                    }
                }
                element_key(left, left_type).cmp(&element_key(right, right_type))
            }
        }
    })
}

fn parameter_context(expr: &Expr) -> bool {
    match expr {
        Expr::Param(_) | Expr::Null => true,
        Expr::Function(function) if function.name.eq_ignore_ascii_case("coalesce") => {
            function.expressions().all(parameter_context)
        }
        Expr::Case {
            branches,
            else_expr,
            ..
        } => {
            branches.iter().all(|(_, value)| parameter_context(value))
                && else_expr.as_deref().is_none_or(parameter_context)
        }
        _ => false,
    }
}
