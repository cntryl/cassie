//! Runtime-profile helpers over existing primitive carriers.
use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};
use crate::types::DataType;
pub(super) const FUNCTIONS: &[ScalarFunction] = &[
    ScalarFunction {
        id: FunctionId::PgColumnSize,
        name: "pg_column_size",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Int,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
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
    ScalarFunction {
        id: FunctionId::PgColumnToastChunkId,
        name: "pg_column_toast_chunk_id",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::ObjectId(crate::types::ObjectIdKind::Oid),
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgColumnToastChunkId,
        name: "pg_catalog.pg_column_toast_chunk_id",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::ObjectId(crate::types::ObjectIdKind::Oid),
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgSettingsGetFlags,
        name: "pg_settings_get_flags",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::TextArray,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgSettingsGetFlags,
        name: "pg_catalog.pg_settings_get_flags",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::TextArray,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgRelationFilenode,
        name: "pg_relation_filenode",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::ObjectId(crate::types::ObjectIdKind::Oid),
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgRelationFilenode,
        name: "pg_catalog.pg_relation_filenode",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::ObjectId(crate::types::ObjectIdKind::Oid),
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgRelationFilepath,
        name: "pg_relation_filepath",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgRelationFilepath,
        name: "pg_catalog.pg_relation_filepath",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgFilenodeRelation,
        name: "pg_filenode_relation",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::ObjectId(crate::types::ObjectIdKind::RegClass),
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgFilenodeRelation,
        name: "pg_catalog.pg_filenode_relation",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::ObjectId(crate::types::ObjectIdKind::RegClass),
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgMyTempSchema,
        name: "pg_my_temp_schema",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::ObjectId(crate::types::ObjectIdKind::Oid),
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgMyTempSchema,
        name: "pg_catalog.pg_my_temp_schema",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::ObjectId(crate::types::ObjectIdKind::Oid),
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgIsOtherTempSchema,
        name: "pg_is_other_temp_schema",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Boolean,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgIsOtherTempSchema,
        name: "pg_catalog.pg_is_other_temp_schema",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Boolean,
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
            | FunctionId::PgColumnSize
            | FunctionId::PgColumnToastChunkId
            | FunctionId::PgSettingsGetFlags
            | FunctionId::PgRelationFilenode
            | FunctionId::PgRelationFilepath
            | FunctionId::PgFilenodeRelation
            | FunctionId::PgMyTempSchema
            | FunctionId::PgIsOtherTempSchema
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
