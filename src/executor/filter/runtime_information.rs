//! Truthful capabilities of the current statically linked Cassie runtime.
use super::{require_arg_count, QueryError, Value};
pub(super) fn evaluate(name: &str, args: &[Value]) -> Option<Result<Value, QueryError>> {
    let name = name.strip_prefix("pg_catalog.").unwrap_or(name);
    let expected = match name {
        "pg_is_in_recovery" | "pg_jit_available" | "pg_numa_available" | "unicode_version" => 0,
        "pg_column_compression" => 1,
        _ => return None,
    };
    Some(
        require_arg_count(name, args, expected).map(|()| match name {
            "unicode_version" => {
                let (major, minor, patch) = std::char::UNICODE_VERSION;
                Value::String(format!("{major}.{minor}.{patch}"))
            }
            // SQL values are materialized ordinary carriers, not compressed PG
            // varlena datums. Midge's physical block codecs are a separate property.
            "pg_column_compression" => Value::Null,
            // Ready single-node queries have no archive-standby, JIT or NUMA runtime.
            _ => Value::Bool(false),
        }),
    )
}
