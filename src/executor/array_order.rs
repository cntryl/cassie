//! Declared ARRAY ordering without changing JSON values or storage encoding.
use std::cmp::Ordering;
use std::collections::HashMap;
use std::ops::ControlFlow;

use crate::catalog::FunctionMeta;
use crate::sql::ast::Expr;
use crate::types::{DataType, Schema, Value};

use super::batch::RowAccess;
use super::semantic::SemanticValue;

pub(super) mod sort_numeric;
pub(super) mod sort_vector;

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
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<SemanticValue, crate::app::CassieError> {
    if let Some(key) = super::search_lineage::extract(row, expr, controls)? {
        return Ok(key);
    }
    key_after_lineage(row, expr, value, functions, controls)
}

fn key_after_lineage<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    value: &Value,
    functions: &HashMap<String, FunctionMeta>,
    controls: Option<&crate::runtime::QueryExecutionControls>,
) -> Result<SemanticValue, crate::app::CassieError> {
    let declared = match expr {
        Expr::Column(name) => row.column_type(name),
        Expr::Cast { data_type, .. } => Some(data_type),
        _ => None,
    };
    if let Some(declared) = declared {
        if matches!(declared, DataType::Char { .. }) || super::semantic::jsonb_type(declared) {
            return super::semantic::typed_value(value, Some(declared), controls);
        }
        return Ok(typed_key(value, Some(declared)));
    }
    if matches!(expr, Expr::Column(_)) {
        return Ok(SemanticValue::from_value(value));
    }
    let inferred = expression_type(row, expr, functions);
    if inferred.as_ref().is_some_and(super::semantic::jsonb_type) {
        return super::semantic::typed_value(value, inferred.as_ref(), controls);
    }
    Ok(typed_key(value, inferred.as_ref()))
}

pub(super) fn sort_vector_key<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    value: &Value,
    functions: &HashMap<String, FunctionMeta>,
    expected: &DataType,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(SemanticValue, Option<usize>), crate::app::CassieError> {
    if let Some(key) = super::search_lineage::extract(row, expr, Some(controls))? {
        return Ok((key, None));
    }
    let matches = match expr {
        Expr::Column(name) => row.column_type(name) == Some(expected),
        Expr::Cast { data_type, .. } => data_type == expected,
        _ => expression_type(row, expr, functions).as_ref() == Some(expected),
    };
    if matches {
        let (key, retained) = sort_vector::build(value, controls)?;
        return Ok((key, Some(retained)));
    }
    key_after_lineage(row, expr, value, functions, Some(controls)).map(|key| (key, None))
}

pub(super) fn sort_numeric_key<R: RowAccess + ?Sized>(
    row: &R,
    expr: &Expr,
    value: &Value,
    functions: &HashMap<String, FunctionMeta>,
    expected: &DataType,
    controls: &crate::runtime::QueryExecutionControls,
) -> Result<(SemanticValue, Option<usize>), crate::app::CassieError> {
    if let Some(key) = super::search_lineage::extract(row, expr, Some(controls))? {
        return Ok((key, None));
    }
    let matches = match expr {
        Expr::Column(name) => row.column_type(name) == Some(expected),
        Expr::Cast { data_type, .. } => data_type == expected,
        _ => expression_type(row, expr, functions).as_ref() == Some(expected),
    };
    if matches {
        let (key, retained) = sort_numeric::build(value, controls)?;
        return Ok((key, Some(retained)));
    }
    key_after_lineage(row, expr, value, functions, Some(controls)).map(|key| (key, None))
}

fn typed_key(value: &Value, data_type: Option<&DataType>) -> SemanticValue {
    let Some(DataType::Array(element_type)) = data_type else {
        return SemanticValue::from_value(value);
    };
    match value {
        Value::Array(array) if matches!(element_type.as_ref(), DataType::Vector(_)) => {
            SemanticValue::SqlArray {
                dimensions: array
                    .dimensions()
                    .iter()
                    .map(|dimension| (dimension.length, dimension.lower_bound))
                    .collect(),
                elements: array
                    .elements()
                    .iter()
                    .map(|element| match element {
                        Value::Vector(vector) => SemanticValue::Array(
                            vector
                                .values
                                .iter()
                                .map(|value| {
                                    SemanticValue::from_value(&Value::Float64(f64::from(*value)))
                                })
                                .collect(),
                        ),
                        value => SemanticValue::from_value(value),
                    })
                    .collect(),
            }
        }
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
    if matches!(data_type, DataType::Json | DataType::Jsonb) && !value.is_null() {
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
    let left = array_value(left.1, &left_type)?;
    let right = array_value(right.1, &right_type)?;
    crate::executor::filter::compare_sql_arrays(&left, &right)
}

fn array_value(value: &Value, element_type: &DataType) -> Option<crate::types::SqlArray> {
    match value {
        Value::Array(array) => Some(array.clone()),
        Value::Json(value) => {
            crate::types::SqlArray::from_nested_json_typed(value, element_type).ok()
        }
        Value::String(text) => serde_json::from_str(text)
            .ok()
            .and_then(|value| {
                crate::types::SqlArray::from_nested_json_typed(&value, element_type).ok()
            })
            .or_else(|| {
                let value = crate::types::array::parse_text_array(text, element_type).ok()?;
                crate::types::SqlArray::from_nested_json_typed(&value, element_type).ok()
            }),
        _ => None,
    }
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
