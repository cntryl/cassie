//! PostgreSQL setting-flag metadata independent of Cassie's supported GUC set.

use super::{QueryError, Value};

pub(super) fn evaluate(name: &str, args: &[Value]) -> Option<Result<Value, QueryError>> {
    if !matches!(
        name,
        "pg_settings_get_flags" | "pg_catalog.pg_settings_get_flags"
    ) {
        return None;
    }
    Some(super::require_arg_count(name, args, 1).and_then(|()| {
        match args.first() {
            Some(Value::String(setting)) if setting.eq_ignore_ascii_case("work_mem") => {
                Ok(Value::Json(serde_json::json!(["EXPLAIN"])))
            }
            Some(Value::Null | Value::String(_)) => Ok(Value::Null),
            _ => Err(crate::app::CassieError::Execution(
                "function pg_settings_get_flags(text) does not exist".into(),
            )
            .into()),
        }
    }))
}
