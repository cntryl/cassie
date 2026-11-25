//! Truthful PostgreSQL relation-location policy for Midge-backed relations.

use super::{QueryError, Value};

pub(super) fn evaluate(name: &str, args: &[Value]) -> Option<Result<Value, QueryError>> {
    let expected = match name {
        "pg_relation_filenode"
        | "pg_catalog.pg_relation_filenode"
        | "pg_relation_filepath"
        | "pg_catalog.pg_relation_filepath" => 1,
        "pg_filenode_relation" | "pg_catalog.pg_filenode_relation" => 2,
        _ => return None,
    };
    Some(
        super::require_arg_count(name, args, expected).and_then(|()| {
            if args.iter().any(|argument| matches!(argument, Value::Null)) {
                return Ok(Value::Null);
            }
            Err(crate::app::CassieError::Unsupported(
                "PostgreSQL relation filenodes and file paths are not available for Midge storage"
                    .into(),
            )
            .into())
        }),
    )
}
