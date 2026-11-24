pub mod aggregate_core;
pub mod any_value;
pub mod arrays;
pub mod bitwise;
pub mod bytea;
pub mod catalog;
pub mod collection_aggregates;
pub mod enums;
pub mod graph_types;
pub mod graph_weighted;
pub mod json;
pub mod math;
pub mod object_identity;
pub mod ordered_set;
pub mod pattern_matching;
pub mod regression;
pub mod retrieval;
pub mod runtime;
pub mod sequences;
pub mod session_catalog;
pub mod statistical;
pub mod temporal;
pub mod text;
pub mod window;

pub use bytea::{
    bit_input_selection, has_distinct_builtin_arity, is_bytea_function, BitInputSelection,
};
pub use math::MathFunction;
pub use object_identity::is_identify_object;
pub use object_identity::lookup_kind as object_lookup_kind;
pub use pattern_matching::QUANTIFIED_PATTERN_FUNCTION;
pub use runtime::{is_loaded_modules, is_runtime_helper, loaded_modules_columns};
pub use sequences::SEQUENCE_FUNCTION_NAMES;
pub use temporal::{
    binary_type as temporal_binary_type, clock_dependent as temporal_clock_dependent,
    return_type as temporal_return_type,
};
pub use window::{aggregate_window_function, window_function, WindowArgumentType, WindowFunction};

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
    PgcryptoDigest,
    Count,
    Grouping,
    Sum,
    Avg,
    VarPop,
    VarSamp,
    Variance,
    StddevPop,
    StddevSamp,
    Stddev,
    Corr,
    CovarPop,
    CovarSamp,
    RegrCount,
    RegrSlope,
    RegrIntercept,
    RegrR2,
    RegrSxx,
    RegrSxy,
    RegrSyy,
    RegrAvgx,
    RegrAvgy,
    Min,
    Max,
    StringAgg,
    CollectionAggregate(CollectionAggregate),
    BoolAnd,
    BoolOr,
    BitAnd,
    BitOr,
    BitXor,
    Mode,
    PercentileCont,
    PercentileDisc,
    Rank,
    DenseRank,
    PercentRank,
    CumeDist,
    Lag,
    Lead,
    NthValue,
    AnyValue,
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
    PgGetFunctionArguments,
    PgGetFunctionIdentityArguments,
    PgGetFunctionResult,
    PgInputIsValid,
    PgMyTempSchema,
    PgIsOtherTempSchema,
    HasDatabasePrivilege,
    PgBackendPid,
    CurrentQuery,
    CurrentSchema,
    CurrentSchemas,
    CurrentDatabase,
    CurrentCatalog,
    PgClientEncoding,
    CurrentUser,
    SessionUser,
    CurrentRole,
    QuoteIdent,
    QuoteNullable,
    FormatType,
    PgGetExpr,
    PgGetIndexDef,
    PgGetConstraintDef,
    PgGetViewDef,
    PgGetUserById,
    ObjDescription,
    ColDescription,
    HasSchemaPrivilege,
    HasTablePrivilege,
    PgTableIsVisible,
    Length,
    Cardinality,
    ArrayLength,
    ArrayCat,
    ArrayAppend,
    ArrayPrepend,
    ArrayFill,
    EnumFirst,
    EnumLast,
    EnumRange,
    GraphKeyPart,
    GraphKeyPartTagged,
    GraphVertexRef,
    GraphEdgeRef,
    GraphPathToJsonb,
    ArrayToString,
    StringToArray,
    ArrayDims,
    ArrayLower,
    ArrayUpper,
    ArrayPosition,
    ArrayPositions,
    ArrayNdims,
    ArrayReverse,
    ArraySort,
    ArrayShuffle,
    ArraySample,
    TrimArray,
    Lower,
    Upper,
    Substring,
    Trim,
    Ltrim,
    Rtrim,
    SplitPart,
    Strpos,
    Replace,
    Format,
    Left,
    Right,
    Overlay,
    Lpad,
    Rpad,
    Repeat,
    Reverse,
    Initcap,
    Translate,
    ConcatWs,
    OctetLength,
    BitLength,
    Ascii,
    Chr,
    RegexpLike,
    SimilarToEscape,
    QuantifiedPattern,
    RegexpCount,
    RegexpReplace,
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
    PgShowAllSettings,
    PgIsInRecovery,
    PgJitAvailable,
    PgNumaAvailable,
    UnicodeVersion,
    PgColumnCompression,
    PgColumnSize,
    PgColumnToastChunkId,
    PgSettingsGetFlags,
    PgRelationFilenode,
    PgRelationFilepath,
    PgFilenodeRelation,
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
    PgSleep,
    PgSleepFor,
    PgSleepUntil,
    Json(json::JsonFunction),
    Math(MathFunction),
    /// A date/time builtin; its result type depends on its arguments.
    Temporal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollectionAggregate {
    Array,
    Json {
        binary: bool,
        strict: bool,
    },
    JsonObject {
        binary: bool,
        strict: bool,
        unique: bool,
    },
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
    Json,
    Jsonb,
    Bytea,
    Float,
    Text,
    Name,
    TextArray,
    IntArray,
    Int,
    BigInt,
    Boolean,
    FirstNonNullArgument,
    FirstComparedArgument,
    NumericArgument,
    SumArgument,
    Timestamp,
    ObjectId(crate::types::ObjectIdKind),
    NameArray,
    /// Resolved from the argument types by [`math::result_type`].
    Math,
    Unknown,
}

impl FunctionReturnType {
    /// Returns the fixed scalar type, if this return policy does not depend on arguments.
    #[must_use]
    pub fn fixed_type(self) -> Option<crate::types::DataType> {
        use crate::types::DataType;
        Some(match self {
            Self::Json => DataType::Json,
            Self::Jsonb => DataType::Jsonb,
            Self::Bytea => DataType::Bytea,
            Self::Float => DataType::Float,
            Self::Text => DataType::Text,
            Self::TextArray => DataType::Array(Box::new(DataType::Text)),
            Self::IntArray => DataType::Array(Box::new(DataType::Int)),
            Self::Name => DataType::Name,
            Self::Int => DataType::Int,
            Self::BigInt => DataType::BigInt,
            Self::Boolean => DataType::Boolean,
            Self::Timestamp => DataType::Timestamp,
            Self::ObjectId(kind) => DataType::ObjectId(kind),
            Self::NameArray => DataType::Array(Box::new(DataType::Name)),
            Self::FirstNonNullArgument
            | Self::FirstComparedArgument
            | Self::NumericArgument
            | Self::SumArgument
            | Self::Math
            | Self::Unknown => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionArity {
    Exact(usize),
    AtLeast(usize),
    Range { min: usize, max: usize },
    OneOf(&'static [usize]),
}

impl FunctionArity {
    #[must_use]
    pub fn matches(self, actual: usize) -> bool {
        match self {
            FunctionArity::Exact(expected) => actual == expected,
            FunctionArity::AtLeast(min) => actual >= min,
            FunctionArity::Range { min, max } => (min..=max).contains(&actual),
            FunctionArity::OneOf(values) => values.contains(&actual),
        }
    }

    #[must_use]
    pub fn describe(self) -> String {
        match self {
            FunctionArity::Exact(expected) => format!("{expected} args"),
            FunctionArity::AtLeast(min) => format!("at least {min} args"),
            FunctionArity::Range { min, max } => format!("{min} to {max} args"),
            FunctionArity::OneOf(values) => {
                values
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(" or ")
                    + " args"
            }
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

/// Whether a name selects one of Cassie's FROM-only catalog table functions.
#[must_use]
pub fn is_catalog_table(name: &str) -> bool {
    runtime::is_loaded_modules(name)
        || is_options_table(name)
        || is_pg_input_error_info(name)
        || object_identity::is_identify_object(name)
        || graph_types::is_table_function(name)
        || graph_weighted::is_weighted_paths(name)
}

/// Whether `name` is a selected options-expansion function.
#[must_use]
pub fn is_options_table(name: &str) -> bool {
    name.eq_ignore_ascii_case("pg_options_to_table")
        || name.eq_ignore_ascii_case("pg_catalog.pg_options_to_table")
}

/// Whether `name` is the selected input-error diagnostic table function.
#[must_use]
pub fn is_pg_input_error_info(name: &str) -> bool {
    name.eq_ignore_ascii_case("pg_input_error_info")
        || name.eq_ignore_ascii_case("pg_catalog.pg_input_error_info")
}

/// Whether `name` is a PostgreSQL server filesystem helper that Cassie rejects explicitly.
#[must_use]
pub fn is_server_file_helper(name: &str) -> bool {
    let name = name
        .strip_prefix("pg_catalog.")
        .unwrap_or(name)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "pg_ls_archive_statusdir"
            | "pg_ls_dir"
            | "pg_ls_logdir"
            | "pg_ls_logicalmapdir"
            | "pg_ls_logicalsnapdir"
            | "pg_ls_replslotdir"
            | "pg_ls_summariesdir"
            | "pg_ls_tmpdir"
            | "pg_ls_waldir"
            | "pg_read_binary_file"
            | "pg_read_file"
            | "pg_stat_file"
    )
}

#[cfg(test)]
mod tests {
    use super::{function, registry, FunctionArity, FunctionId};
    use std::collections::HashSet;

    #[test]
    fn should_preserve_function_registry_identity() {
        // Arrange
        let mut names = HashSet::new();

        // Act
        let functions = registry();

        // Assert
        for function in functions {
            assert!(names.insert(function.name.to_ascii_lowercase()));
        }
        assert!(function("lag").is_none());
        assert_eq!(
            function("regexp_replace").map(|metadata| metadata.id),
            Some(FunctionId::RegexpReplace)
        );
        assert_eq!(
            function("count").map(|metadata| metadata.arity),
            Some(FunctionArity::Exact(1))
        );
    }
}

fn scalar_functions() -> impl Iterator<Item = &'static ScalarFunction> {
    arrays::FUNCTIONS
        .iter()
        .chain(POWER_FUNCTION)
        .chain(aggregate_core::FUNCTIONS)
        .chain(retrieval::FUNCTIONS)
        .chain(session_catalog::FUNCTIONS)
        .chain(SCALAR_FUNCTIONS)
}

#[must_use]
pub fn registry() -> Vec<ScalarFunction> {
    scalar_functions()
        .chain(any_value::FUNCTIONS)
        .chain(bitwise::FUNCTIONS)
        .chain(ordered_set::FUNCTIONS)
        .chain(statistical::FUNCTIONS)
        .chain(regression::FUNCTIONS)
        .chain(collection_aggregates::FUNCTIONS)
        .chain(text::FUNCTIONS)
        .chain(pattern_matching::FUNCTIONS)
        .chain(bytea::FUNCTIONS)
        .chain(catalog::FUNCTIONS)
        .chain(object_identity::FUNCTIONS)
        .chain(sequences::FUNCTIONS)
        .chain(runtime::FUNCTIONS)
        .chain(json::FUNCTIONS)
        .chain(math::FUNCTIONS)
        .chain(enums::FUNCTIONS)
        .chain(graph_types::FUNCTIONS)
        .chain(temporal::SLEEP_FUNCTIONS)
        .cloned()
        .collect()
}

#[must_use]
pub fn function(name: &str) -> Option<&'static ScalarFunction> {
    scalar_functions()
        .chain(any_value::FUNCTIONS)
        .chain(bitwise::FUNCTIONS)
        .chain(ordered_set::FUNCTIONS)
        .chain(statistical::FUNCTIONS)
        .chain(regression::FUNCTIONS)
        .chain(collection_aggregates::FUNCTIONS)
        .chain(text::FUNCTIONS)
        .chain(pattern_matching::FUNCTIONS)
        .chain(bytea::FUNCTIONS)
        .chain(catalog::FUNCTIONS)
        .chain(object_identity::FUNCTIONS)
        .chain(sequences::FUNCTIONS)
        .chain(runtime::FUNCTIONS)
        .chain(json::FUNCTIONS)
        .chain(math::FUNCTIONS)
        .chain(enums::FUNCTIONS)
        .chain(graph_types::FUNCTIONS)
        .chain(temporal::FUNCTIONS)
        .chain(temporal::SLEEP_FUNCTIONS)
        .find(|metadata| metadata.name.eq_ignore_ascii_case(name))
        .or_else(|| qualified_function(name))
}

fn qualified_function(name: &str) -> Option<&'static ScalarFunction> {
    if is_qualified_digest(name) {
        return bytea::FUNCTIONS
            .iter()
            .find(|metadata| metadata.id == FunctionId::PgcryptoDigest);
    }
    name.get(..11)
        .filter(|prefix| prefix.eq_ignore_ascii_case("pg_catalog."))
        .and_then(|_| name.get(11..))
        .filter(|local| {
            [
                "pg_show_all_settings",
                "substring",
                "trim",
                "power",
                "abs",
                "string_agg",
                "array_agg",
                "json_agg",
                "json_agg_strict",
                "jsonb_agg",
                "jsonb_agg_strict",
                "json_object_agg",
                "json_object_agg_strict",
                "json_object_agg_unique",
                "json_object_agg_unique_strict",
                "jsonb_object_agg",
                "jsonb_object_agg_strict",
                "jsonb_object_agg_unique",
                "jsonb_object_agg_unique_strict",
                "any_value",
                "cardinality",
                "array_length",
                "array_cat",
                "array_append",
                "array_prepend",
                "array_fill",
                "array_to_string",
                "string_to_array",
                "array_dims",
                "array_lower",
                "array_upper",
                "array_position",
                "array_positions",
                "array_ndims",
                "array_reverse",
                "array_sort",
                "array_shuffle",
                "array_sample",
                "trim_array",
            ]
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(local))
                || runtime::FUNCTIONS
                    .iter()
                    .chain(temporal::FUNCTIONS)
                    .any(|metadata| metadata.name.eq_ignore_ascii_case(local))
                || catalog::FUNCTIONS
                    .iter()
                    .chain(object_identity::FUNCTIONS)
                    .chain(sequences::FUNCTIONS)
                    .any(|metadata| metadata.name.eq_ignore_ascii_case(local))
                || text::FUNCTIONS
                    .iter()
                    .any(|metadata| metadata.name.eq_ignore_ascii_case(local))
                || pattern_matching::FUNCTIONS
                    .iter()
                    .any(|metadata| metadata.name.eq_ignore_ascii_case(local))
                || bytea::FUNCTIONS
                    .iter()
                    .chain(json::FUNCTIONS)
                    .chain(math::FUNCTIONS)
                    .any(|metadata| metadata.name.eq_ignore_ascii_case(local))
                || regression::FUNCTIONS
                    .iter()
                    .any(|metadata| metadata.name.eq_ignore_ascii_case(local))
                || bitwise::FUNCTIONS
                    .iter()
                    .any(|metadata| metadata.name.eq_ignore_ascii_case(local))
        })
        .and_then(|local| {
            scalar_functions()
                .chain(any_value::FUNCTIONS)
                .chain(bitwise::FUNCTIONS)
                .chain(statistical::FUNCTIONS)
                .chain(regression::FUNCTIONS)
                .chain(collection_aggregates::FUNCTIONS)
                .chain(text::FUNCTIONS)
                .chain(pattern_matching::FUNCTIONS)
                .chain(bytea::FUNCTIONS)
                .chain(catalog::FUNCTIONS)
                .chain(object_identity::FUNCTIONS)
                .chain(sequences::FUNCTIONS)
                .chain(runtime::FUNCTIONS)
                .chain(json::FUNCTIONS)
                .chain(math::FUNCTIONS)
                .chain(temporal::FUNCTIONS)
                .chain(temporal::SLEEP_FUNCTIONS)
                .find(|metadata| metadata.name.eq_ignore_ascii_case(local))
        })
}

fn is_qualified_digest(name: &str) -> bool {
    crate::catalog::scope::parse_name(name).is_ok_and(|parsed| {
        matches!(
            parsed,
            crate::catalog::scope::ParsedName::SchemaQualified { name, .. }
                | crate::catalog::scope::ParsedName::DatabaseQualified { name, .. }
                if name.eq_ignore_ascii_case("digest")
        )
    })
}

const POWER_FUNCTION: &[ScalarFunction] = &[ScalarFunction {
    id: FunctionId::Power,
    name: "power",
    arity: FunctionArity::Exact(2),
    return_type: FunctionReturnType::Float,
    is_aggregate: false,
    bypass_arity_validation: false,
}];

const SCALAR_FUNCTIONS: &[ScalarFunction] = &[
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
        id: FunctionId::PgShowAllSettings,
        name: "pg_show_all_settings",
        arity: FunctionArity::Exact(0),
        return_type: FunctionReturnType::Unknown,
        is_aggregate: false,
        bypass_arity_validation: true,
    },
];
