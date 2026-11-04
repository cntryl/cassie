//! Schema-aware row materialization for controlled time-series reads.
use super::{
    json_to_query_value, payload_field, BatchRow, CollectionSchema, DataType, DocumentRef, Value,
};

pub(super) fn document_to_row(
    document: &DocumentRef,
    fields: &[String],
    schema: Option<&CollectionSchema>,
) -> BatchRow {
    let schema_has_id = schema.is_some_and(CollectionSchema::declares_id);
    let mut values = Vec::with_capacity(fields.len() + 2);
    crate::executor::scan::push_row_identity(&mut values, &document.id);
    for field in fields {
        if crate::types::row_identity::is_identity_reference(field, schema_has_id) {
            continue;
        }
        let value = payload_field(&document.payload, field)
            .map_or(Value::Null, |value| typed_value(value, schema, field));
        values.push((field.clone(), value));
    }
    let mut row = BatchRow::from_projected_values(values);
    crate::executor::scan::attach_row_types(&mut row, schema);
    row
}

fn typed_value(value: &serde_json::Value, schema: Option<&CollectionSchema>, field: &str) -> Value {
    match schema
        .and_then(|schema| {
            schema.fields.iter().find(|entry| {
                crate::sql::ColumnIdentifierPath::matches_stored_field(field, &entry.name)
            })
        })
        .map(|entry| &entry.data_type)
    {
        Some(kind @ DataType::Array(_)) => crate::executor::scan::json_to_typed_value(value, kind),
        Some(DataType::Int | DataType::BigInt) => value
            .as_i64()
            .map_or_else(|| json_to_query_value(value), Value::Int64),
        Some(DataType::Float) => value
            .as_f64()
            .map_or_else(|| json_to_query_value(value), Value::Float64),
        Some(DataType::Boolean) => value
            .as_bool()
            .map_or_else(|| json_to_query_value(value), Value::Bool),
        Some(data_type @ (DataType::Date | DataType::Time | DataType::Timestamp)) => {
            crate::types::extended::temporal_column_value(value, data_type)
                .map_or_else(|| json_to_query_value(value), Value::Typed)
        }
        _ => json_to_query_value(value),
    }
}
