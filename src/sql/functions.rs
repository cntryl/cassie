mod constant_folding;
pub(crate) mod primitive_casts;
pub(crate) use constant_folding::is_immutable_scalar;
mod catalog;
mod datum_size;
mod numeric_identity;
mod object_identity;
mod sequences;
pub(crate) use numeric_identity::has_unselected_numeric;
pub(crate) use object_identity::lookup_kind as object_lookup_kind;
pub(crate) use sequences::SEQUENCE_FUNCTION_NAMES;
mod runtime;
pub(crate) use datum_size::{datum_size_kind, is_column_size, DatumSizeKind};
mod table;
pub(crate) use runtime::{is_loaded_modules, is_runtime_helper, loaded_modules_columns};
pub(crate) use table::{catalog_table_columns, is_catalog_table, is_options_table};
mod bytea;
pub(crate) mod hashing;
mod inputs;
pub(crate) use bytea::{
    bit_input_selection, has_distinct_builtin_arity, is_bytea_function, BitInputSelection,
};
mod text;
pub(crate) use inputs::{
    aggregate_accepts_input, has_distinct_equality, input_signature, FunctionInputSignature,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionId {
    Encode,
    Decode,
    GetByte,
    SetByte,
    GetBit,
    SetBit,
    BitCount,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
    Count,
    Sum,
    Avg,
    Min,
    Max,
    BoolAnd,
    BoolOr,
    Search,
    SearchScore,
    VectorDistance,
    VectorScore,
    CosineDistance,
    DotProduct,
    HybridScore,
    Snippet,
    Version,
    CassieVersion,
    CurrentSetting,
    SetConfig,
    PgEncodingToChar,
    PgCharToEncoding,
    PgSizeBytes,
    PgSizePretty,
    HasDatabasePrivilege,
    PgBackendPid,
    CurrentQuery,
    CurrentSchema,
    CurrentDatabase,
    CurrentUser,
    SessionUser,
    CurrentRole,
    QuoteIdent,
    FormatType,
    PgGetExpr,
    PgGetUserById,
    ObjDescription,
    HasSchemaPrivilege,
    HasTablePrivilege,
    PgTableIsVisible,
    Length,
    Lower,
    Upper,
    Substring,
    Trim,
    Ltrim,
    Rtrim,
    SplitPart,
    Strpos,
    Left,
    Right,
    OctetLength,
    BitLength,
    Ascii,
    Chr,
    StartsWith,
    NumNulls,
    NumNonNulls,
    Concat,
    Coalesce,
    NullIf,
    Greatest,
    Least,
    Abs,
    Power,
    TimeBucket,
    GraphNeighbors,
    GraphExpand,
    GraphShortestPath,
    PgShowAllSettings,
    PgIsInRecovery,
    PgJitAvailable,
    PgNumaAvailable,
    UnicodeVersion,
    PgColumnCompression,
    PgColumnSize,
    PgTypeof,
    ToRegClass,
    ToRegCollation,
    ToRegNamespace,
    ToRegOper,
    ToRegOperator,
    ToRegProc,
    ToRegProcedure,
    ToRegRole,
    ToRegType,
    Nextval,
    Currval,
    Setval,
    Lastval,
    PgGetSerialSequence,
}

#[derive(Debug, Clone)]
pub struct ScalarFunction {
    pub id: FunctionId,
    pub name: &'static str,
    pub arity: FunctionArity,
    pub return_type: FunctionReturnType,
    pub is_aggregate: bool,
    pub bypass_arity_validation: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionReturnType {
    Bytea,
    Float,
    Text,
    Int,
    BigInt,
    Boolean,
    FirstNonNullArgument,
    FirstComparedArgument,
    NumericArgument,
    SumArgument,
    Timestamp,
    ObjectId(crate::types::ObjectIdKind),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionArity {
    Exact(usize),
    AtLeast(usize),
    Range { min: usize, max: usize },
}

impl FunctionArity {
    #[must_use]
    pub fn matches(self, actual: usize) -> bool {
        match self {
            FunctionArity::Exact(expected) => actual == expected,
            FunctionArity::AtLeast(min) => actual >= min,
            FunctionArity::Range { min, max } => (min..=max).contains(&actual),
        }
    }

    #[must_use]
    pub fn describe(self) -> String {
        match self {
            FunctionArity::Exact(expected) => format!("{expected} args"),
            FunctionArity::AtLeast(min) => format!("at least {min} args"),
            FunctionArity::Range { min, max } => format!("{min} to {max} args"),
        }
    }
}

#[must_use]
pub fn is_aggregate_function(name: &str) -> bool {
    function(name).is_some_and(|metadata| metadata.is_aggregate)
}

#[must_use]
pub fn aggregate_arity(name: &str) -> Option<usize> {
    let metadata = function(name)?;
    if !metadata.is_aggregate {
        return None;
    }
    let FunctionArity::Exact(arity) = metadata.arity else {
        return None;
    };
    Some(arity)
}

#[must_use]
pub fn registry() -> Vec<ScalarFunction> {
    SCALAR_FUNCTIONS
        .iter()
        .chain(text::FUNCTIONS)
        .chain(bytea::FUNCTIONS)
        .chain(catalog::FUNCTIONS)
        .chain(object_identity::FUNCTIONS)
        .chain(sequences::FUNCTIONS)
        .chain(runtime::FUNCTIONS)
        .cloned()
        .collect()
}

#[must_use]
pub fn function(name: &str) -> Option<&'static ScalarFunction> {
    SCALAR_FUNCTIONS
        .iter()
        .chain(text::FUNCTIONS)
        .chain(bytea::FUNCTIONS)
        .chain(catalog::FUNCTIONS)
        .chain(object_identity::FUNCTIONS)
        .chain(sequences::FUNCTIONS)
        .chain(runtime::FUNCTIONS)
        .find(|metadata| metadata.name.eq_ignore_ascii_case(name))
        .or_else(|| {
            name.get(..11)
                .filter(|prefix| prefix.eq_ignore_ascii_case("pg_catalog."))
                .and_then(|_| name.get(11..))
                .filter(|local| {
                    ["pg_show_all_settings", "substring", "trim", "power", "abs"]
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(local))
                        || runtime::FUNCTIONS
                            .iter()
                            .any(|metadata| metadata.name.eq_ignore_ascii_case(local))
                        || catalog::FUNCTIONS
                            .iter()
                            .chain(object_identity::FUNCTIONS)
                            .chain(sequences::FUNCTIONS)
                            .any(|metadata| metadata.name.eq_ignore_ascii_case(local))
                        || text::FUNCTIONS
                            .iter()
                            .any(|metadata| metadata.name.eq_ignore_ascii_case(local))
                        || bytea::FUNCTIONS
                            .iter()
                            .any(|metadata| metadata.name.eq_ignore_ascii_case(local))
                })
                .and_then(|local| {
                    SCALAR_FUNCTIONS
                        .iter()
                        .chain(text::FUNCTIONS)
                        .chain(bytea::FUNCTIONS)
                        .chain(catalog::FUNCTIONS)
                        .chain(object_identity::FUNCTIONS)
                        .chain(sequences::FUNCTIONS)
                        .chain(runtime::FUNCTIONS)
                        .find(|metadata| metadata.name.eq_ignore_ascii_case(local))
                })
        })
}

const SCALAR_FUNCTIONS: &[ScalarFunction] = &[
    ScalarFunction {
        id: FunctionId::Power,
        name: "power",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Float,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Count,
        name: "count",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::BigInt,
        is_aggregate: true,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Sum,
        name: "sum",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::SumArgument,
        is_aggregate: true,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Avg,
        name: "avg",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Float,
        is_aggregate: true,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Min,
        name: "min",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::FirstNonNullArgument,
        is_aggregate: true,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Max,
        name: "max",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::FirstNonNullArgument,
        is_aggregate: true,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::BoolAnd,
        name: "bool_and",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Boolean,
        is_aggregate: true,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::BoolAnd,
        name: "pg_catalog.bool_and",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Boolean,
        is_aggregate: true,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::BoolOr,
        name: "bool_or",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Boolean,
        is_aggregate: true,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::BoolOr,
        name: "pg_catalog.bool_or",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Boolean,
        is_aggregate: true,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::BoolAnd,
        name: "every",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Boolean,
        is_aggregate: true,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::BoolAnd,
        name: "pg_catalog.every",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Boolean,
        is_aggregate: true,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Search,
        name: "search",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Boolean,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::SearchScore,
        name: "search_score",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Float,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::VectorDistance,
        name: "vector_distance",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Float,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::VectorScore,
        name: "vector_score",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Float,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::CosineDistance,
        name: "cosine_distance",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Float,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::DotProduct,
        name: "dot_product",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Float,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::HybridScore,
        name: "hybrid_score",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Float,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Snippet,
        name: "snippet",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Version,
        name: "version",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Version,
        name: "pg_catalog.version",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::CassieVersion,
        name: "cassie_version",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::CurrentSetting,
        name: "current_setting",
        arity: FunctionArity::Range { min: 1, max: 2 },
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::CurrentSetting,
        name: "pg_catalog.current_setting",
        arity: FunctionArity::Range { min: 1, max: 2 },
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::SetConfig,
        name: "set_config",
        arity: FunctionArity::Exact(3),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::SetConfig,
        name: "pg_catalog.set_config",
        arity: FunctionArity::Exact(3),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgEncodingToChar,
        name: "pg_encoding_to_char",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgEncodingToChar,
        name: "pg_catalog.pg_encoding_to_char",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::HasDatabasePrivilege,
        name: "has_database_privilege",
        arity: FunctionArity::Range { min: 2, max: 3 },
        return_type: FunctionReturnType::Boolean,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::HasDatabasePrivilege,
        name: "pg_catalog.has_database_privilege",
        arity: FunctionArity::Range { min: 2, max: 3 },
        return_type: FunctionReturnType::Boolean,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgBackendPid,
        name: "pg_backend_pid",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Int,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgBackendPid,
        name: "pg_catalog.pg_backend_pid",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Int,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::CurrentSchema,
        name: "current_schema",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::CurrentSchema,
        name: "pg_catalog.current_schema",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::CurrentDatabase,
        name: "current_database",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::CurrentUser,
        name: "current_user",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::SessionUser,
        name: "session_user",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::CurrentRole,
        name: "current_role",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::QuoteIdent,
        name: "quote_ident",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::QuoteIdent,
        name: "pg_catalog.quote_ident",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::FormatType,
        name: "format_type",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::FormatType,
        name: "pg_catalog.format_type",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgGetExpr,
        name: "pg_get_expr",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgGetExpr,
        name: "pg_catalog.pg_get_expr",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgGetUserById,
        name: "pg_get_userbyid",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgGetUserById,
        name: "pg_catalog.pg_get_userbyid",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::ObjDescription,
        name: "obj_description",
        arity: FunctionArity::Range { min: 1, max: 2 },
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::ObjDescription,
        name: "pg_catalog.obj_description",
        arity: FunctionArity::Range { min: 1, max: 2 },
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::HasSchemaPrivilege,
        name: "has_schema_privilege",
        arity: FunctionArity::Range { min: 2, max: 3 },
        return_type: FunctionReturnType::Boolean,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::HasSchemaPrivilege,
        name: "pg_catalog.has_schema_privilege",
        arity: FunctionArity::Range { min: 2, max: 3 },
        return_type: FunctionReturnType::Boolean,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::HasTablePrivilege,
        name: "has_table_privilege",
        arity: FunctionArity::Range { min: 2, max: 3 },
        return_type: FunctionReturnType::Boolean,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::HasTablePrivilege,
        name: "pg_catalog.has_table_privilege",
        arity: FunctionArity::Range { min: 2, max: 3 },
        return_type: FunctionReturnType::Boolean,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgTableIsVisible,
        name: "pg_table_is_visible",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Boolean,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgTableIsVisible,
        name: "pg_catalog.pg_table_is_visible",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Boolean,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Length,
        name: "length",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Int,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Length,
        name: "len",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Int,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Lower,
        name: "lower",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Upper,
        name: "upper",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Substring,
        name: "substring",
        arity: FunctionArity::Range { min: 2, max: 3 },
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Trim,
        name: "trim",
        arity: FunctionArity::Range { min: 1, max: 2 },
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Concat,
        name: "concat",
        arity: FunctionArity::AtLeast(1),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Coalesce,
        name: "coalesce",
        arity: FunctionArity::AtLeast(1),
        return_type: FunctionReturnType::FirstNonNullArgument,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::NullIf,
        name: "nullif",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::FirstComparedArgument,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Greatest,
        name: "greatest",
        arity: FunctionArity::AtLeast(1),
        return_type: FunctionReturnType::FirstNonNullArgument,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Least,
        name: "least",
        arity: FunctionArity::AtLeast(1),
        return_type: FunctionReturnType::FirstNonNullArgument,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Abs,
        name: "abs",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::NumericArgument,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::TimeBucket,
        name: "time_bucket",
        arity: FunctionArity::Range { min: 2, max: 3 },
        return_type: FunctionReturnType::Timestamp,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::GraphNeighbors,
        name: "graph_neighbors",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Unknown,
        is_aggregate: false,
        bypass_arity_validation: true,
    },
    ScalarFunction {
        id: FunctionId::GraphExpand,
        name: "graph_expand",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Unknown,
        is_aggregate: false,
        bypass_arity_validation: true,
    },
    ScalarFunction {
        id: FunctionId::GraphShortestPath,
        name: "graph_shortest_path",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Unknown,
        is_aggregate: false,
        bypass_arity_validation: true,
    },
    ScalarFunction {
        id: FunctionId::PgShowAllSettings,
        name: "pg_show_all_settings",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Unknown,
        is_aggregate: false,
        bypass_arity_validation: true,
    },
];

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::{function, registry, FunctionArity, FunctionId, FunctionReturnType};

    #[test]
    fn should_define_one_metadata_mapping_per_function_id() {
        // Arrange
        let functions = registry();
        let mut names = HashSet::new();
        let mut mappings = HashMap::new();

        // Act
        for metadata in functions {
            assert!(names.insert(metadata.name.to_ascii_lowercase()));
            let mapping = (metadata.return_type, metadata.arity);
            if let Some(existing) = mappings.insert(metadata.id, mapping) {
                assert_eq!(
                    existing, mapping,
                    "divergent metadata for {:?}",
                    metadata.id
                );
            }
        }

        // Assert
        assert!(!mappings.is_empty());
        assert!(mappings.values().all(|(return_type, arity)| {
            !matches!(return_type, FunctionReturnType::Unknown)
                || matches!(arity, FunctionArity::Exact(_))
        }));
    }

    #[test]
    fn should_resolve_supported_qualified_function_alias() {
        // Arrange
        let qualified_name = "pg_catalog.pg_show_all_settings";

        // Act
        let metadata = function(qualified_name).expect("qualified alias should resolve");

        // Assert
        assert_eq!(metadata.id, FunctionId::PgShowAllSettings);
        assert!(metadata.bypass_arity_validation);
    }
}
