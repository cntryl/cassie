use std::sync::Arc;

use crate::app::{Cassie, CassieSession};
use crate::executor::ColumnMeta;
use crate::pgwire::protocol::{RowDescriptionField, ServerMessage};
use crate::types::Value;

pub fn run_simple_query(
    cassie: &Arc<Cassie>,
    session: &CassieSession,
    sql: &str,
    params: Vec<Value>,
) -> Vec<ServerMessage> {
    match cassie.execute_sql(session, sql, params) {
        Ok(result) => {
            let mut out = Vec::new();
            out.push(ServerMessage::RowDescription(
                result
                    .columns
                    .into_iter()
                    .map(RowDescriptionField::from)
                    .collect(),
            ));
            for row in result.rows {
                out.push(ServerMessage::DataRow(
                    row.into_iter().map(format_value).collect(),
                ));
            }
            out.push(ServerMessage::CommandComplete(result.command));
            out
        }
        Err(err) => vec![ServerMessage::ErrorResponse(err.to_string())],
    }
}

pub fn describe_query(
    cassie: &Cassie,
    sql: &str,
) -> Result<Vec<ColumnMeta>, crate::app::CassieError> {
    cassie.describe_sql(sql)
}

pub fn parse_bind_param(raw: &str) -> Value {
    let raw = raw.trim();
    if raw.eq_ignore_ascii_case("null") {
        return Value::Null;
    }
    if raw.eq_ignore_ascii_case("true") {
        return Value::Bool(true);
    }
    if raw.eq_ignore_ascii_case("false") {
        return Value::Bool(false);
    }
    if raw.len() >= 2 {
        if raw.starts_with('\'') && raw.ends_with('\'') {
            return Value::String(raw[1..raw.len() - 1].to_string());
        }
        if raw.starts_with('"') && raw.ends_with('"') {
            return Value::String(raw[1..raw.len() - 1].to_string());
        }
    }
    if let Ok(value) = raw.parse::<i64>() {
        return Value::Int64(value);
    }
    if let Ok(value) = raw.parse::<f64>() {
        return Value::Float64(value);
    }
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Some(items) = json.as_array() {
            let mut values = Vec::new();
            for value in items {
                if let Some(number) = value.as_f64() {
                    values.push(number as f32);
                } else {
                    return Value::Json(json);
                }
            }
            return Value::Vector(crate::types::Vector { values });
        }
        return Value::Json(json);
    }

    Value::String(raw.to_string())
}

fn format_value(value: Value) -> String {
    match value {
        Value::Null => "NULL".to_string(),
        Value::Bool(v) => v.to_string(),
        Value::Int64(v) => v.to_string(),
        Value::Float64(v) => v.to_string(),
        Value::String(v) => v,
        Value::Vector(v) => format!(
            "[{}]",
            v.values
                .iter()
                .map(|item| item.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Json(v) => v.to_string(),
    }
}
