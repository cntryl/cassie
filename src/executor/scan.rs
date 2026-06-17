use crate::app::Cassie;
use crate::executor::batch::{Batch, BatchRow, DEFAULT_BATCH_SIZE};
use crate::types::Value;

pub(crate) async fn scan(
    cassie: &Cassie,
    collection: &str,
) -> Result<Vec<Batch>, crate::executor::QueryError> {
    let document_batches = cassie
        .midge
        .scan_documents_batched(collection, DEFAULT_BATCH_SIZE)
        .await
        .map_err(|e| crate::executor::QueryError::General(e.to_string()))?;

    Ok(document_batches
        .into_iter()
        .map(|documents| {
            documents
                .into_iter()
                .map(|document| {
                    let mut row = Vec::new();
                    row.push(("id".to_string(), Value::String(document.id)));
                    if let Some(obj) = document.payload.as_object() {
                        for (k, v) in obj.iter() {
                            row.push((k.clone(), json_to_value(v)));
                        }
                    }
                    BatchRow::new(row)
                })
                .collect::<Batch>()
        })
        .collect())
}

fn json_to_value(value: &serde_json::Value) -> Value {
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
