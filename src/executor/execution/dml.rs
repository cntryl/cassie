use super::dml_referential_actions;
use super::{
    aggregate, batch, check_timeout, ensure_query_memory_budget, execute_plan, filter, projection,
    reserve_projection_output_before_building, scan, BatchRow, Cassie, CassieSession,
    CollectionSchema, ColumnMeta, CteContext, DataType, Expr, FieldMeta, FunctionMeta, HashMap,
    InsertSource, LogicalPlan, QueryError, QueryExecutionControls, QueryResult, QuerySource,
    SelectItem, Value,
};
use crate::types::row_identity::ROW_IDENTITY_COLUMN;

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

const NON_FINITE_WRITE_VALUE_ERROR: &str = "stored float values must be finite";

fn value_to_json(value: &Value) -> Result<serde_json::Value, QueryError> {
    Ok(match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(value) => serde_json::Value::Bool(*value),
        Value::Int64(value) => serde_json::Value::Number((*value).into()),
        Value::Float64(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| QueryError::General(NON_FINITE_WRITE_VALUE_ERROR.to_string()))?,
        Value::String(value) => serde_json::Value::String(value.clone()),
        Value::Vector(value) => serde_json::Value::Array(
            value
                .values
                .iter()
                .map(|value| {
                    serde_json::Number::from_f64((*value).into())
                        .map(serde_json::Value::Number)
                        .ok_or_else(|| {
                            QueryError::General(NON_FINITE_WRITE_VALUE_ERROR.to_string())
                        })
                })
                .collect::<Result<_, _>>()?,
        ),
        Value::Json(value) => value.clone(),
    })
}

fn value_to_json_for_field(
    field: &str,
    value: &Value,
    data_type: &DataType,
) -> Result<serde_json::Value, QueryError> {
    if let (DataType::Json, Value::String(text)) = (data_type, value) {
        return serde_json::from_str(text).map_err(|error| {
            crate::app::CassieError::Parse(format!("field '{field}' expects JSON: {error}")).into()
        });
    }

    if let (DataType::Vector(dimensions), Value::String(text)) = (data_type, value) {
        return vector_literal_to_json(field, text, *dimensions);
    }

    value_to_json(value)
}

/// Coerces a pgvector-style text literal such as `'[1,2,3]'` into a JSON array.
///
/// A literal that is not a vector of the declared width stays a string so schema
/// validation reports the mismatch; a component outside the f32 range is a range error.
fn vector_literal_to_json(
    field: &str,
    text: &str,
    dimensions: usize,
) -> Result<serde_json::Value, QueryError> {
    use crate::vector::text::{visit_components, TextVectorError};

    let range_error = || {
        QueryError::General(format!(
            "field '{field}' vector element is outside f32 range"
        ))
    };
    match visit_components(text, dimensions, |_| {}) {
        Ok(()) => {}
        Err(TextVectorError::Shape) => return Ok(serde_json::Value::String(text.to_string())),
        Err(TextVectorError::Range) => return Err(range_error()),
    }
    // Validation has completed before allocating the exact known-width output.
    // Each callback receives a finite f32; its f64 promotion is a JSON number.
    let mut components = Vec::with_capacity(dimensions);
    visit_components(text, dimensions, |component| {
        components.push(serde_json::Value::from(f64::from(component)));
    })
    .map_err(|_| range_error())?;
    Ok(serde_json::Value::Array(components))
}

fn update_assignment_to_json(
    field: &str,
    value: &Value,
    schema: &CollectionSchema,
) -> Result<serde_json::Value, QueryError> {
    if let Some(field_meta) = schema
        .fields
        .iter()
        .find(|candidate| candidate.name == field)
    {
        if matches!(
            field_meta.data_type,
            DataType::SmallInt | DataType::Int | DataType::BigInt
        ) {
            if let Value::Float64(number) = value {
                if let Some(integer) = integral_json_number(*number) {
                    return Ok(serde_json::Value::Number(integer));
                }
            }
        }
    }

    if let Some(field_meta) = schema
        .fields
        .iter()
        .find(|candidate| candidate.name == field)
    {
        return value_to_json_for_field(field, value, &field_meta.data_type);
    }

    value_to_json(value)
}

fn inserted_row_to_batch_row(
    row_id: &str,
    schema: &CollectionSchema,
    payload: &serde_json::Value,
) -> BatchRow {
    let mut row = Vec::with_capacity(schema.fields.len() + 1);
    row.push((
        ROW_IDENTITY_COLUMN.to_string(),
        Value::String(row_id.to_string()),
    ));

    for field in &schema.fields {
        let value = payload.get(&field.name).map_or(Value::Null, |value| {
            // JSON document-only scalars stay exact. A missing key remains
            // SQL NULL, as INSERT/UPDATE omit SQL-null JSON fields.
            if matches!(field.data_type, DataType::Json)
                && crate::types::json::requires_document_carrier(value)
            {
                Value::Json(value.clone())
            } else {
                json_to_value(value)
            }
        });
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
    aggregate::normalize_dml_returning_identity_columns(
        &mut columns,
        returning,
        schema.is_some_and(CollectionSchema::declares_id),
    );
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
    controls: &'a QueryExecutionControls,
    statement_read_controls: &'a QueryExecutionControls,
}

fn build_dml_result(
    context: &DmlResultContext<'_>,
    affected_count: usize,
    mut returning_rows: Vec<BatchRow>,
) -> Result<QueryResult, QueryError> {
    if context.returning.is_empty() {
        return Ok(QueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            command: format!("{} {affected_count}", context.command_prefix),
        });
    }
    let column_schema = context.cassie.catalog.get_schema(context.table);
    let has_arrays = column_schema.as_ref().is_some_and(|schema| {
        schema
            .fields
            .iter()
            .any(|field| matches!(field.data_type, DataType::Array(_)))
    });
    if has_arrays {
        if let Some(first) = returning_rows.first_mut() {
            scan::attach_row_types(first, column_schema.as_ref());
            let types = first.shared_data_types();
            if let Some(types) = types {
                for row in &mut returning_rows[1..] {
                    row.set_data_types(types.clone());
                }
            }
        }
    }
    let _returning_memory = if has_arrays {
        Some((
            ensure_query_memory_budget(context.controls, std::slice::from_ref(&returning_rows))?,
            reserve_projection_output_before_building(
                context.controls,
                std::slice::from_ref(&returning_rows),
                context.returning,
            )?,
        ))
    } else {
        None
    };
    let projected = if super::exists_projection::contains(context.returning) {
        let source = QuerySource::Collection(
            crate::sql::IdentifierPath::parse(context.table).map_err(QueryError::General)?,
        );
        let env = super::source::SourceExecutionEnv {
            cassie: context.cassie,
            session: context.session,
            user_functions: context.user_functions,
            params: context.params,
            controls: context.statement_read_controls,
        };
        let (_read_scope, _overlay_scope) =
            crate::executor::execution::entrypoints::enter_statement_read(
                context.statement_read_controls,
            );
        let mut ctes = CteContext::new();
        super::exists_projection::project(
            &env,
            &mut ctes,
            &source,
            vec![returning_rows],
            context.returning,
            None,
        )?
        .into_iter()
        .flatten()
        .collect()
    } else {
        projection::project_rows(
            returning_rows,
            context.returning,
            context.params,
            None,
            context.user_functions,
            context.session,
        )?
    };
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
    match row.get(ROW_IDENTITY_COLUMN) {
        Some(Value::String(value)) if !value.is_empty() => Ok(value.clone()),
        _ => Err(QueryError::General(
            "scanned row is missing internal row id".to_string(),
        )),
    }
}
