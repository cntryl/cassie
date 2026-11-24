//! Truthful capabilities of the current statically linked Cassie runtime.
use super::{require_arg_count, CassieSession, QueryError, Value};
pub(super) fn evaluate(
    name: &str,
    args: &[Value],
    session: Option<&CassieSession>,
) -> Option<Result<Value, QueryError>> {
    let name = name.strip_prefix("pg_catalog.").unwrap_or(name);
    let expected = match name {
        "pg_is_in_recovery" | "pg_jit_available" | "pg_numa_available" | "pg_my_temp_schema"
        | "unicode_version" | "current_query" => 0,
        "pg_is_other_temp_schema" | "pg_column_compression" => 1,
        _ => return None,
    };
    Some(require_arg_count(name, args, expected).map(|()| {
        match name {
            "current_query" => session
                .and_then(CassieSession::statement_text)
                .map_or(Value::Null, |sql| Value::String(sql.to_owned())),
            "unicode_version" => {
                let (major, minor, patch) = std::char::UNICODE_VERSION;
                Value::String(format!("{major}.{minor}.{patch}"))
            }
            // SQL values are materialized ordinary carriers, not compressed PG
            // varlena datums. Midge's physical block codecs are a separate property.
            "pg_column_compression" => Value::Null,
            // Temporary relations and their namespaces are not part of the
            // selected Cassie session catalog, so no session has one.
            "pg_my_temp_schema" => Value::Int64(0),
            // Ready single-node queries have no archive-standby, JIT or NUMA runtime.
            _ => Value::Bool(false),
        }
    }))
}
