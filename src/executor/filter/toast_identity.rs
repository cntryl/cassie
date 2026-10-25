//! Truthful TOAST identity helpers for Midge-backed row storage.

use super::{QueryError, Value};

pub(super) fn evaluate(name: &str, args: &[Value]) -> Option<Result<Value, QueryError>> {
    matches!(
        name,
        "pg_column_toast_chunk_id" | "pg_catalog.pg_column_toast_chunk_id"
    )
    .then(|| super::require_arg_count(name, args, 1).map(|()| Value::Null))
}
