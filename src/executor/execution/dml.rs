use super::dml_referential_actions;
use super::{
    aggregate, batch, check_timeout, ensure_query_memory_budget, execute_plan, filter, projection,
    scan, BatchRow, Cassie, CassieSession, CollectionSchema, ColumnMeta, CteContext, DataType,
    Expr, FieldMeta, FunctionMeta, HashMap, InsertSource, LogicalPlan, QueryError,
    QueryExecutionControls, QueryResult, QuerySource, SelectItem, Value,
};

#[path = "dml_delete.rs"]
mod dml_delete;
#[path = "dml_insert.rs"]
mod dml_insert;
#[path = "dml_update.rs"]
mod dml_update;

pub(super) use dml_delete::execute_delete;
pub(super) use dml_insert::execute_insert;
pub(crate) use dml_insert::resolve_transaction_conflict_intents;
pub(super) use dml_update::execute_update;

fn value_to_json(value: &Value) -> serde_json::Value {
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(value) => serde_json::Value::Bool(*value),
        Value::Int64(value) => serde_json::Value::Number((*value).into()),
        Value::Float64(value) => serde_json::Number::from_f64(*value)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::String(value) => serde_json::Value::String(value.clone()),
        Value::Vector(value) => serde_json::Value::Array(
            value
                .values
                .iter()
                .filter_map(|value| serde_json::Number::from_f64((*value).into()))
                .map(serde_json::Value::Number)
                .collect(),
        ),
        Value::Json(value) => value.clone(),
    }
}

fn update_assignment_to_json(
    field: &str,
    value: &Value,
    schema: &CollectionSchema,
) -> serde_json::Value {
    if let Some(field_meta) = schema
        .fields
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(field))
    {
        if let DataType::Vector(dimensions) = &field_meta.data_type {
            if let Some(text) = value.as_str() {
                if let Some(vector) = super::scored::parse_vector_literal(text) {
                    if vector.len() == *dimensions {
                        return serde_json::Value::Array(
                            vector
                                .into_iter()
                                .map(|component| {
                                    serde_json::Number::from_f64(f64::from(component))
                                        .map(serde_json::Value::Number)
                                })
                                .collect::<Option<Vec<_>>>()
                                .unwrap_or_default(),
                        );
                    }
                }
            }
        }
        if matches!(
            field_meta.data_type,
            DataType::SmallInt | DataType::Int | DataType::BigInt
        ) {
            if let Value::Float64(number) = value {
                if let Some(integer) = integral_json_number(*number) {
                    return serde_json::Value::Number(integer);
                }
            }
        }
    }

    value_to_json(value)
}

fn inserted_row_to_batch_row(
    row_id: &str,
    schema: &CollectionSchema,
    payload: &serde_json::Value,
) -> BatchRow {
    let mut row = Vec::with_capacity(schema.fields.len() + 1);
    row.push(("_id".to_string(), Value::String(row_id.to_string())));

    for field in &schema.fields {
        let value = payload.get(&field.name).map_or(Value::Null, json_to_value);
        row.push((field.name.clone(), value));
    }

    BatchRow::new(row)
}

fn dml_returning_columns(
    returning: &[SelectItem],
    schema: Option<&CollectionSchema>,
    user_functions: &HashMap<String, FunctionMeta>,
) -> Vec<ColumnMeta> {
    let mut columns = aggregate::columns_from_projection(returning, schema, user_functions);
    if returning
        .iter()
        .any(|item| matches!(item, SelectItem::Wildcard))
    {
        for column in &mut columns {
            if column.name == "id" {
                column.name = "_id".to_string();
                break;
            }
        }
    }
    columns
}

fn json_to_value(value: &serde_json::Value) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    if let Some(value) = value.as_str() {
        return Value::String(value.to_string());
    }
    if let Some(value) = value.as_bool() {
        return Value::Bool(value);
    }
    if let Some(value) = value.as_i64() {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_u64().and_then(|value| i64::try_from(value).ok()) {
        return Value::Int64(value);
    }
    if let Some(value) = value.as_f64() {
        return Value::Float64(value);
    }
    Value::Json(value.clone())
}

fn integral_json_number(value: f64) -> Option<serde_json::Number> {
    if !value.is_finite() || value.fract() != 0.0 {
        return None;
    }
    format!("{value:.0}").parse::<i64>().ok().map(Into::into)
}

struct DmlResultContext<'a> {
    cassie: &'a Cassie,
    session: Option<&'a CassieSession>,
    table: &'a str,
    returning: &'a [SelectItem],
    params: &'a [Value],
    user_functions: &'a HashMap<String, FunctionMeta>,
    command_prefix: &'a str,
}

fn build_dml_result(
    context: &DmlResultContext<'_>,
    affected_count: usize,
    returning_rows: Vec<BatchRow>,
) -> Result<QueryResult, QueryError> {
    if context.returning.is_empty() {
        return Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: format!("{} {affected_count}", context.command_prefix),
        });
    }
    let projected = projection::project_rows(
        returning_rows,
        context.returning,
        context.params,
        None,
        context.user_functions,
        context.session,
    )?;
    let column_schema = context.cassie.catalog.get_schema(context.table);
    let columns = dml_returning_columns(
        context.returning,
        column_schema.as_ref(),
        context.user_functions,
    );
    Ok(QueryResult {
        columns,
        rows: projected.into_iter().map(BatchRow::into_values).collect(),
        command: format!("{} {affected_count}", context.command_prefix),
    })
}

fn row_id_from_batch_row(row: &BatchRow) -> Result<String, QueryError> {
    match row.get("id") {
        Some(Value::String(value)) if !value.is_empty() => Ok(value.clone()),
        _ => Err(QueryError::General(
            "scanned row is missing internal row id".to_string(),
        )),
    }
}
