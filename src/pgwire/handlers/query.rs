use std::sync::Arc;

use crate::app::{Cassie, CassieSession};
use crate::pgwire::protocol::{RowDescriptionField, ServerMessage};
use crate::types::Value;

/// Runs a simple query and renders its result as text frames.
///
/// Benchmark-facing only (`benches/support/workloads/pgwire.rs`); the served
/// simple-query path lives in `pgwire::connection`. Keep protocol fixes in
/// the live path rather than here.
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

/// # Errors
///
/// Returns an error when validation, storage, or execution fails.
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
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Json(v) => v.to_string(),
    }
}
