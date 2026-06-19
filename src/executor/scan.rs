use crate::app::{Cassie, CassieSession};
use crate::executor::batch::{Batch, BatchRow, DEFAULT_BATCH_SIZE};
use crate::types::Value;
use std::collections::HashSet;

pub(crate) async fn scan(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    collection: &str,
) -> Result<Vec<Batch>, crate::executor::QueryError> {
    let document_batches = cassie
        .scan_documents_batched_for_session(session, collection, DEFAULT_BATCH_SIZE)
        .await
        .map_err(|error| {
            cassie.runtime.record_storage_access("data", false, false);
            crate::executor::QueryError::General(error.to_string())
        })?;
    cassie.runtime.record_storage_access("data", false, true);
    let schema = cassie.catalog.get_schema(collection).await;

    Ok(document_batches
        .into_iter()
        .map(|documents| {
            documents
                .into_iter()
                .map(|document| {
                    let mut row = Vec::new();
                    row.push(("id".to_string(), Value::String(document.id)));
                    if let Some(obj) = document.payload.as_object() {
                        if let Some(schema) = schema.as_ref() {
                            let mut seen = HashSet::new();
                            for field in &schema.fields {
                                let value = obj
                                    .get(&field.name)
                                    .map(json_to_value)
                                    .unwrap_or(Value::Null);
                                row.push((field.name.clone(), value));
                                seen.insert(field.name.clone());
                            }
                            for (k, v) in obj.iter() {
                                if !seen.contains(k) {
                                    row.push((k.clone(), json_to_value(v)));
                                }
                            }
                        } else {
                            for (k, v) in obj.iter() {
                                row.push((k.clone(), json_to_value(v)));
                            }
                        }
                    }
                    BatchRow::new(row)
                })
                .collect::<Batch>()
        })
        .collect())
}

pub(crate) async fn scan_projected(
    cassie: &Cassie,
    session: Option<&CassieSession>,
    collection: &str,
    fields: &[String],
) -> Result<Vec<Batch>, crate::executor::QueryError> {
    let document_batches = cassie
        .scan_projected_documents_batched_for_session(
            session,
            collection,
            DEFAULT_BATCH_SIZE,
            fields,
        )
        .await
        .map_err(|error| {
            cassie.runtime.record_storage_access("data", false, false);
            crate::executor::QueryError::General(error.to_string())
        })?;
    cassie.runtime.record_storage_access("data", false, true);

    Ok(document_batches
        .into_iter()
        .map(|documents| {
            documents
                .into_iter()
                .map(|document| {
                    let mut row = Vec::with_capacity(fields.len() + 1);
                    row.push(("id".to_string(), Value::String(document.id)));
                    let object = document.payload.as_object();
                    for field in fields {
                        let value = object
                            .and_then(|object| projected_field_value(object, field))
                            .map(json_to_value)
                            .unwrap_or(Value::Null);
                        row.push((field.clone(), value));
                    }
                    BatchRow::new(row)
                })
                .collect::<Batch>()
        })
        .collect())
}

fn projected_field_value<'a>(
    object: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<&'a serde_json::Value> {
    object
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(field))
        .map(|(_, value)| value)
}

fn json_to_value(value: &serde_json::Value) -> Value {
    if value.is_null() {
        return Value::Null;
    }
    if let Some(v) = value.as_str() {
        return Value::String(v.to_string());
    }
    if let Some(v) = value.as_bool() {
        return Value::Bool(v);
    }
    if let Some(v) = value.as_i64() {
        return Value::Int64(v);
    }
    if let Some(v) = value.as_u64().and_then(|v| i64::try_from(v).ok()) {
        return Value::Int64(v);
    }
    if let Some(v) = value.as_f64() {
        return Value::Float64(v);
    }
    Value::Json(value.clone())
}
