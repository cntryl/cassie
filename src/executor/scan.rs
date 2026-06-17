use crate::app::Cassie;
use crate::types::Value;

pub async fn scan(
    cassie: &Cassie,
    collection: &str,
) -> Result<Vec<Vec<(String, Value)>>, crate::executor::QueryError> {
    let documents = cassie
        .midge
        .all_fields_json(collection)
        .await
        .map_err(|e| crate::executor::QueryError::General(e.to_string()))?;

    let mut rows = Vec::with_capacity(documents.len());
    for (id, payload) in documents {
        let mut row = Vec::new();
        row.push(("id".to_string(), Value::String(id)));
        if let Some(obj) = payload.as_object() {
            for (k, v) in obj.iter() {
                row.push((k.clone(), json_to_value(v)));
            }
        }
        rows.push(row);
    }
    Ok(rows)
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
