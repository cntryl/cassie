use serde::{Deserialize, Serialize};

use super::{check_timeout, retention, QueryError, QueryExecutionControls, RetainedRows};
use crate::executor::semantic::SemanticValue;
use crate::sql::ast::{
    CteCycle, CteSearch, CteSearchOrder, Expr, ParsedStatement, QueryStatement, SelectItem,
};
use crate::types::{DataType, FieldSchema, Schema, Value};

#[cfg(test)]
#[path = "search_cycle_tests.rs"]
mod tests;

const LINEAGE_COLUMN_PREFIX: &str = "\0cassie_cte_lineage:";

#[derive(Debug, Default, Serialize, Deserialize)]
struct Lineage {
    search_path: Vec<Vec<Value>>,
    cycle_path: Vec<Vec<Value>>,
    cycle: bool,
    #[serde(default)]
    search_column: Option<String>,
    #[serde(default)]
    search_order: Option<CteSearchOrder>,
}

fn lineage_column_name(cte_name: &str) -> String {
    format!("{LINEAGE_COLUMN_PREFIX}{cte_name}")
}

pub(super) fn add_lineage_projection(
    statement: &ParsedStatement,
    cte_name: &str,
    search: Option<&CteSearch>,
    cycle: Option<&CteCycle>,
) -> Result<ParsedStatement, QueryError> {
    if search.is_none() && cycle.is_none() {
        return Ok(statement.clone());
    }
    let mut statement = statement.clone();
    let QueryStatement::Select(select) = &mut statement.statement else {
        return Err(QueryError::General(
            "recursive SEARCH and CYCLE require a SELECT recursive term".into(),
        ));
    };
    if select.set.is_some() {
        return Err(QueryError::General(
            "recursive SEARCH and CYCLE do not support a set operation in the recursive term"
                .into(),
        ));
    }
    select.projection.push(SelectItem::Column {
        name: lineage_column_name(cte_name),
        alias: Some(lineage_column_name(cte_name)),
    });
    Ok(statement)
}

pub(super) fn working_fields(
    fields: &[FieldSchema],
    cte_name: &str,
    search: Option<&CteSearch>,
    cycle: Option<&CteCycle>,
) -> Vec<FieldSchema> {
    let mut fields = fields.to_vec();
    if search.is_some() || cycle.is_some() {
        fields.push(FieldSchema {
            name: lineage_column_name(cte_name),
            data_type: DataType::Text,
            nullable: false,
        });
    }
    fields
}

pub(super) fn append_base_metadata(
    rows: &mut RetainedRows,
    cte_name: &str,
    search: Option<&CteSearch>,
    cycle: Option<&CteCycle>,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    if search.is_none() && cycle.is_none() {
        return Ok(());
    }
    reserve_expansion(rows, search, cycle)?;
    let default_value = cycle
        .map(cycle_values)
        .transpose()?
        .map(|(_, default)| default);
    let added_columns = usize::from(search.is_some()) + 2 * usize::from(cycle.is_some()) + 1;
    for row in &mut rows.rows {
        check_timeout(controls)?;
        let mut lineage = Lineage::default();
        if let Some(search) = search {
            lineage.search_column = Some(search.sequence_column.clone());
            lineage.search_order = Some(search.order);
            let keys = key_values(row, &search.by)?;
            lineage.search_path.push(keys.clone());
            row.try_reserve(added_columns)
                .map_err(|error| retention::allocation(&error))?;
            row.push((
                search.sequence_column.clone(),
                Value::String(search_value(search, &lineage, &keys)),
            ));
        } else {
            row.try_reserve(added_columns)
                .map_err(|error| retention::allocation(&error))?;
        }
        if let Some(cycle) = cycle {
            let keys = key_values(row, &cycle.by)?;
            lineage.cycle_path.push(keys);
            row.push((
                cycle.mark_column.clone(),
                clone_value(default_value.as_ref())?,
            ));
            row.push((
                cycle.path_column.clone(),
                Value::String(record_array(&lineage.cycle_path)),
            ));
        }
        row.push((lineage_column_name(cte_name), encode_lineage(&lineage)?));
    }
    shrink_expansion(rows)?;
    Ok(())
}

pub(super) fn append_recursive_metadata(
    rows: &mut RetainedRows,
    cte_name: &str,
    search: Option<&CteSearch>,
    cycle: Option<&CteCycle>,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    if search.is_none() && cycle.is_none() {
        return Ok(());
    }
    reserve_expansion(rows, search, cycle)?;
    let values = cycle.map(cycle_values).transpose()?;
    let mark_value = values.as_ref().map(|(mark, _)| mark);
    let default_value = values.as_ref().map(|(_, default)| default);
    let added_columns = usize::from(search.is_some()) + 2 * usize::from(cycle.is_some()) + 1;
    for row in &mut rows.rows {
        check_timeout(controls)?;
        let lineage_index = row
            .iter()
            .position(|(name, _)| name == &lineage_column_name(cte_name))
            .ok_or_else(|| QueryError::General("recursive CTE lineage is missing".into()))?;
        let (_, lineage_value) = row.remove(lineage_index);
        let Value::String(lineage_json) = lineage_value else {
            return Err(QueryError::General(
                "recursive CTE lineage has an invalid value".into(),
            ));
        };
        let mut lineage = decode_lineage(&lineage_json)?;
        let search_keys = search
            .map(|search| key_values(row, &search.by))
            .transpose()?;
        if let (Some(search), Some(keys)) = (search, search_keys.as_ref()) {
            lineage.search_column = Some(search.sequence_column.clone());
            lineage.search_order = Some(search.order);
            lineage.search_path.push(keys.clone());
            row.try_reserve(added_columns)
                .map_err(|error| retention::allocation(&error))?;
            row.push((
                search.sequence_column.clone(),
                Value::String(search_value(search, &lineage, keys)),
            ));
        } else {
            row.try_reserve(added_columns)
                .map_err(|error| retention::allocation(&error))?;
        }
        if let Some(cycle) = cycle {
            let keys = key_values(row, &cycle.by)?;
            lineage.cycle = lineage
                .cycle_path
                .iter()
                .any(|previous| same_key(previous, &keys));
            lineage.cycle_path.push(keys);
            row.push((
                cycle.mark_column.clone(),
                if lineage.cycle {
                    clone_value(mark_value)?
                } else {
                    clone_value(default_value)?
                },
            ));
            row.push((
                cycle.path_column.clone(),
                Value::String(record_array(&lineage.cycle_path)),
            ));
        }
        row.push((lineage_column_name(cte_name), encode_lineage(&lineage)?));
    }
    shrink_expansion(rows)?;
    Ok(())
}

pub(super) fn prune_cycle_rows(
    rows: &mut RetainedRows,
    cte_name: &str,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    if rows.rows.is_empty() {
        return Ok(());
    }
    let lineage_column = lineage_column_name(cte_name);
    let mut scratch_bytes = 0usize;
    for row in &rows.rows {
        check_timeout(controls)?;
        let lineage_bytes = row
            .iter()
            .find_map(|(name, value)| {
                (name == &lineage_column).then_some(match value {
                    Value::String(json) => Some(json.len()),
                    _ => None,
                })
            })
            .flatten()
            .ok_or_else(|| QueryError::General("recursive CTE lineage is invalid".into()))?;
        let decoded = crate::runtime::accounted::json::serialized_decode_bytes(lineage_bytes)?;
        scratch_bytes = scratch_bytes
            .checked_add(decoded)
            .ok_or_else(allocation_overflow)?;
    }
    let _lineage_scratch = controls.reserve_query_memory(scratch_bytes)?;
    let mut error = None;
    rows.rows.retain(|row| {
        if error.is_some() {
            return true;
        }
        if let Err(timeout) = check_timeout(controls) {
            error = Some(timeout);
            return true;
        }
        let lineage = row
            .iter()
            .find(|(name, _)| name == &lineage_column)
            .and_then(|(_, value)| match value {
                Value::String(json) => serde_json::from_str::<Lineage>(json).ok(),
                _ => None,
            })
            .ok_or_else(|| QueryError::General("recursive CTE lineage is invalid".into()));
        match lineage {
            Ok(lineage) => !lineage.cycle,
            Err(lineage_error) => {
                error = Some(lineage_error);
                true
            }
        }
    });
    if let Some(error) = error {
        return Err(error);
    }
    rows.memory.shrink_to(retention::rows_bytes(&rows.rows)?);
    Ok(())
}

pub(super) fn remove_lineage(
    rows: &mut RetainedRows,
    cte_name: &str,
    keep_for_search_order: bool,
    controls: &QueryExecutionControls,
) -> Result<(), QueryError> {
    if keep_for_search_order {
        return Ok(());
    }
    let lineage_column = lineage_column_name(cte_name);
    for row in &mut rows.rows {
        check_timeout(controls)?;
        if let Some(index) = row.iter().position(|(name, _)| name == &lineage_column) {
            row.remove(index);
        }
    }
    rows.memory.shrink_to(retention::rows_bytes(&rows.rows)?);
    Ok(())
}

fn reserve_expansion(
    rows: &mut RetainedRows,
    search: Option<&CteSearch>,
    cycle: Option<&CteCycle>,
) -> Result<(), QueryError> {
    let mut extra = 0usize;
    for row in &rows.rows {
        let row_bytes = retention::row_bytes(row)?;
        let field_bytes = (usize::from(search.is_some()) + 3)
            .checked_mul(128)
            .ok_or_else(allocation_overflow)?;
        let allowance = row_bytes
            .checked_mul(if cycle.is_some() { 5 } else { 4 })
            .and_then(|bytes| bytes.checked_add(field_bytes))
            .ok_or_else(allocation_overflow)?;
        extra = extra
            .checked_add(allowance)
            .ok_or_else(allocation_overflow)?;
    }
    rows.memory.try_grow(extra)?;
    Ok(())
}

fn shrink_expansion(rows: &mut RetainedRows) -> Result<(), QueryError> {
    rows.memory.shrink_to(retention::rows_bytes(&rows.rows)?);
    Ok(())
}

fn allocation_overflow() -> QueryError {
    crate::app::CassieError::ResourceLimit("recursive CTE metadata size overflow".into()).into()
}

fn key_values(row: &[(String, Value)], names: &[String]) -> Result<Vec<Value>, QueryError> {
    names
        .iter()
        .map(|name| {
            row.iter()
                .find(|(field, _)| field == name)
                .map(|(_, value)| value.clone())
                .ok_or_else(|| {
                    QueryError::General(format!(
                        "recursive CTE SEARCH or CYCLE key '{name}' is missing"
                    ))
                })
        })
        .collect()
}

fn same_key(left: &[Value], right: &[Value]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            SemanticValue::from_value(left) == SemanticValue::from_value(right)
        })
}

fn search_value(search: &CteSearch, lineage: &Lineage, keys: &[Value]) -> String {
    match search.order {
        CteSearchOrder::DepthFirst => record_array(&lineage.search_path),
        CteSearchOrder::BreadthFirst => {
            let mut values = Vec::with_capacity(keys.len() + 1);
            values.push(Value::Int64(
                i64::try_from(lineage.search_path.len().saturating_sub(1)).unwrap_or(i64::MAX),
            ));
            values.extend_from_slice(keys);
            record_tuple(&values)
        }
    }
}

fn record_array(path: &[Vec<Value>]) -> String {
    let contents = path
        .iter()
        .map(|tuple| array_element(&record_tuple(tuple)))
        .collect::<Vec<_>>()
        .join(",");
    format!("{{{contents}}}")
}

fn record_tuple(values: &[Value]) -> String {
    let contents = values
        .iter()
        .map(record_field)
        .collect::<Vec<_>>()
        .join(",");
    format!("({contents})")
}

fn record_field(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => composite_field(value),
        Value::Bool(value) => if *value { "t" } else { "f" }.to_owned(),
        Value::Int64(value) => value.to_string(),
        Value::Float64(value) => value.to_string(),
        Value::Typed(value) => composite_field(&value.text()),
        Value::Vector(value) => composite_field(
            &value
                .values
                .iter()
                .map(f32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        Value::Json(value) => composite_field(&value.to_string()),
    }
}

fn composite_field(value: &str) -> String {
    let quote = value.is_empty()
        || value.trim() != value
        || value
            .chars()
            .any(|character| matches!(character, ',' | '(' | ')' | '"' | '\\'));
    if !quote {
        return value.to_owned();
    }
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn array_element(value: &str) -> String {
    let quote = value.is_empty()
        || value
            .chars()
            .any(|character| matches!(character, ',' | '{' | '}' | '"' | '\\'));
    if !quote {
        return value.to_owned();
    }
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn encode_lineage(lineage: &Lineage) -> Result<Value, QueryError> {
    serde_json::to_string(lineage)
        .map(Value::String)
        .map_err(|error| {
            QueryError::General(format!("cannot encode recursive CTE lineage: {error}"))
        })
}

fn decode_lineage(json: &str) -> Result<Lineage, QueryError> {
    serde_json::from_str(json)
        .map_err(|error| QueryError::General(format!("invalid recursive CTE lineage: {error}")))
}

fn literal_value(expr: &Expr) -> Result<Value, QueryError> {
    match expr {
        Expr::StringLiteral(value) => Ok(Value::String(value.clone())),
        Expr::IntegerLiteral(value) => Ok(Value::Int64(*value)),
        Expr::NumberLiteral(value) => Ok(Value::Float64(*value)),
        Expr::NumericLiteral(value) => crate::types::extended::numeric::parse_text(value)
            .map(crate::types::typed::TypedValue::Numeric)
            .map(Value::Typed)
            .map_err(|error| QueryError::General(error.message)),
        Expr::BoolLiteral(value) => Ok(Value::Bool(*value)),
        Expr::Null => Ok(Value::Null),
        Expr::Cast {
            expr, data_type, ..
        } => {
            let value = literal_value(expr)?;
            cast_literal(value, data_type)
        }
        _ => Err(QueryError::General(
            "CYCLE mark and default values must be constant literals".into(),
        )),
    }
}

fn clone_value(value: Option<&Value>) -> Result<Value, QueryError> {
    value
        .cloned()
        .ok_or_else(|| QueryError::General("CYCLE mark value is missing".into()))
}

fn cast_literal(value: Value, data_type: &DataType) -> Result<Value, QueryError> {
    coerce_cycle_value(value, data_type)
}

fn cycle_values(cycle: &CteCycle) -> Result<(Value, Value), QueryError> {
    let source = Schema { fields: Vec::new() };
    let functions = std::collections::HashMap::new();
    let mark_type =
        crate::sql::binder::infer_expr_type(&cycle.mark_value, &source, &functions, &[])
            .ok_or_else(|| QueryError::General("cannot infer CYCLE mark type".into()))?;
    let default_type =
        crate::sql::binder::infer_expr_type(&cycle.default_value, &source, &functions, &[])
            .ok_or_else(|| QueryError::General("cannot infer CYCLE default type".into()))?;
    let common_type = match (mark_type, default_type) {
        (DataType::Null, DataType::Null) => DataType::Text,
        (DataType::Null, other) | (other, DataType::Null) => other,
        (mark, default) if mark == default => mark,
        (mark, default) => {
            crate::sql::binder::common_case_type(mark, default).ok_or_else(|| {
                QueryError::General("incompatible CYCLE mark and default types".into())
            })?
        }
    };
    let mark = coerce_cycle_value(literal_value(&cycle.mark_value)?, &common_type)?;
    let default = coerce_cycle_value(literal_value(&cycle.default_value)?, &common_type)?;
    Ok((mark, default))
}

fn coerce_cycle_value(value: Value, data_type: &DataType) -> Result<Value, QueryError> {
    use crate::types::extended::ExtendedType;
    use crate::types::typed::TypedValue;

    match (value, data_type) {
        (Value::Null, _) => Ok(Value::Null),
        (
            Value::String(value),
            DataType::Text | DataType::Char { .. } | DataType::Varchar { .. },
        ) => Ok(Value::String(value)),
        (Value::String(value), DataType::Uuid) => uuid::Uuid::parse_str(&value)
            .map(|value| Value::String(value.to_string()))
            .map_err(|_| QueryError::General("invalid UUID CYCLE mark literal".into())),
        (Value::Bool(value), DataType::Boolean) => Ok(Value::Bool(value)),
        (Value::Int64(value), DataType::SmallInt | DataType::Int | DataType::BigInt) => {
            Ok(Value::Int64(value))
        }
        (Value::Float64(value), DataType::Float) => Ok(Value::Float64(value)),
        (Value::Int64(value), DataType::Float) => num_traits::ToPrimitive::to_f64(&value)
            .map(Value::Float64)
            .ok_or_else(|| QueryError::General("cannot coerce CYCLE value to FLOAT8".into())),
        (Value::Typed(TypedValue::Numeric(value)), DataType::Float) => {
            Ok(Value::Float64(value.to_f64()))
        }
        (Value::Typed(TypedValue::Real(value)), DataType::Float) => {
            Ok(Value::Float64(f64::from(value)))
        }
        (value, DataType::Extended(ExtendedType::Numeric(_))) => {
            let text = match value {
                Value::Int64(value) => value.to_string(),
                Value::Float64(value) => value.to_string(),
                Value::String(value) => value,
                Value::Typed(TypedValue::Numeric(value)) => {
                    return Ok(Value::Typed(TypedValue::Numeric(value)));
                }
                _ => {
                    return Err(QueryError::General(
                        "unsupported CYCLE numeric literal conversion".into(),
                    ));
                }
            };
            crate::types::extended::numeric::parse_text(&text)
                .map(TypedValue::Numeric)
                .map(Value::Typed)
                .map_err(|error| QueryError::General(error.message))
        }
        (Value::Typed(TypedValue::Real(value)), DataType::Extended(ExtendedType::Real)) => {
            Ok(Value::Typed(TypedValue::Real(value)))
        }
        (Value::Int64(value), DataType::Extended(ExtendedType::Real)) => {
            let value = num_traits::ToPrimitive::to_f64(&value)
                .ok_or_else(|| QueryError::General("cannot coerce CYCLE value to REAL".into()))?;
            let real = crate::types::extended::real::from_f64(value, "CYCLE mark")
                .map_err(|error| QueryError::General(error.message))?;
            Ok(Value::Typed(TypedValue::Real(real)))
        }
        (Value::Float64(value), DataType::Extended(ExtendedType::Real)) => {
            let real = crate::types::extended::real::from_f64(value, "CYCLE mark")
                .map_err(|error| QueryError::General(error.message))?;
            Ok(Value::Typed(TypedValue::Real(real)))
        }
        (Value::Typed(TypedValue::Numeric(value)), DataType::Extended(ExtendedType::Real)) => {
            let real = crate::types::extended::real::from_f64(value.to_f64(), "CYCLE mark")
                .map_err(|error| QueryError::General(error.message))?;
            Ok(Value::Typed(TypedValue::Real(real)))
        }
        (value, _) => Err(QueryError::General(format!(
            "unsupported CYCLE mark literal conversion for {value:?}"
        ))),
    }
}
