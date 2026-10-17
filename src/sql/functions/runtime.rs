//! Runtime-profile helpers over existing primitive carriers.
use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};
use crate::types::DataType;
pub(super) const FUNCTIONS: &[ScalarFunction] = &[
    ScalarFunction {
        id: FunctionId::CurrentQuery,
        name: "current_query",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgIsInRecovery,
        name: "pg_is_in_recovery",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Boolean,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgJitAvailable,
        name: "pg_jit_available",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Boolean,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgNumaAvailable,
        name: "pg_numa_available",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Boolean,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::UnicodeVersion,
        name: "unicode_version",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgColumnCompression,
        name: "pg_column_compression",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
];
pub(crate) const fn is_runtime_helper(id: FunctionId) -> bool {
    matches!(
        id,
        FunctionId::CurrentQuery
            | FunctionId::PgIsInRecovery
            | FunctionId::PgJitAvailable
            | FunctionId::PgNumaAvailable
            | FunctionId::UnicodeVersion
            | FunctionId::PgColumnCompression
    )
}
pub(crate) fn is_loaded_modules(name: &str) -> bool {
    name.eq_ignore_ascii_case("pg_get_loaded_modules")
        || name.eq_ignore_ascii_case("pg_catalog.pg_get_loaded_modules")
}
pub(crate) fn loaded_modules_columns() -> Vec<(String, DataType)> {
    ["module_name", "version", "file_name"]
        .into_iter()
        .map(|name| (name.into(), DataType::Text))
        .collect()
}
