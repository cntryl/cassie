#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FunctionId {
    Count,
    Sum,
    Avg,
    Min,
    Max,
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
    HasDatabasePrivilege,
    PgBackendPid,
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
    Concat,
    Coalesce,
    Abs,
    TimeBucket,
    GraphNeighbors,
    GraphExpand,
    GraphShortestPath,
    PgShowAllSettings,
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
    Float,
    Text,
    Int,
    Boolean,
    FirstNonNullArgument,
    NumericArgument,
    Timestamp,
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
    SCALAR_FUNCTIONS.to_vec()
}

#[must_use]
pub fn function(name: &str) -> Option<&'static ScalarFunction> {
    let name = name.to_ascii_lowercase();
    let name = name
        .strip_prefix("pg_catalog.")
        .filter(|local_name| *local_name == "pg_show_all_settings")
        .unwrap_or(&name);
    SCALAR_FUNCTIONS
        .iter()
        .find(|metadata| metadata.name.eq_ignore_ascii_case(name))
}

const SCALAR_FUNCTIONS: &[ScalarFunction] = &[
    ScalarFunction {
        id: FunctionId::Count,
        name: "count",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Int,
        is_aggregate: true,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Sum,
        name: "sum",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Float,
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
        return_type: FunctionReturnType::Text,
        is_aggregate: true,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Max,
        name: "max",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Text,
        is_aggregate: true,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::Search,
        name: "search",
        arity: FunctionArity::Exact(2),
        return_type: FunctionReturnType::Float,
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
        arity: FunctionArity::Exact(1),
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
