#[derive(Debug, Clone)]
pub enum FunctionId {
    Search,
    SearchScore,
    VectorDistance,
    VectorScore,
    CosineDistance,
    DotProduct,
    HybridScore,
    Snippet,
    Version,
    CurrentSchema,
    CurrentDatabase,
    CurrentUser,
    SessionUser,
    CurrentRole,
    Length,
    Lower,
    Upper,
    Substring,
    Trim,
    Concat,
    Coalesce,
    Abs,
}

#[derive(Debug, Clone)]
pub struct ScalarFunction {
    pub id: FunctionId,
    pub name: &'static str,
    pub arity: FunctionArity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionArity {
    Exact(usize),
    AtLeast(usize),
    Range { min: usize, max: usize },
}

impl FunctionArity {
    pub fn matches(self, actual: usize) -> bool {
        match self {
            FunctionArity::Exact(expected) => actual == expected,
            FunctionArity::AtLeast(min) => actual >= min,
            FunctionArity::Range { min, max } => (min..=max).contains(&actual),
        }
    }

    pub fn describe(self) -> String {
        match self {
            FunctionArity::Exact(expected) => format!("{expected} args"),
            FunctionArity::AtLeast(min) => format!("at least {min} args"),
            FunctionArity::Range { min, max } => format!("{min} to {max} args"),
        }
    }
}

pub fn is_aggregate_function(name: &str) -> bool {
    aggregate_arity(name).is_some()
}

pub fn aggregate_arity(name: &str) -> Option<usize> {
    match name.to_ascii_lowercase().as_str() {
        "count" | "sum" | "avg" | "min" | "max" => Some(1),
        _ => None,
    }
}

pub fn registry() -> Vec<ScalarFunction> {
    vec![
        ScalarFunction {
            id: FunctionId::Search,
            name: "search",
            arity: FunctionArity::Exact(2),
        },
        ScalarFunction {
            id: FunctionId::SearchScore,
            name: "search_score",
            arity: FunctionArity::Exact(2),
        },
        ScalarFunction {
            id: FunctionId::VectorDistance,
            name: "vector_distance",
            arity: FunctionArity::Exact(2),
        },
        ScalarFunction {
            id: FunctionId::VectorScore,
            name: "vector_score",
            arity: FunctionArity::Exact(2),
        },
        ScalarFunction {
            id: FunctionId::CosineDistance,
            name: "cosine_distance",
            arity: FunctionArity::Exact(2),
        },
        ScalarFunction {
            id: FunctionId::DotProduct,
            name: "dot_product",
            arity: FunctionArity::Exact(2),
        },
        ScalarFunction {
            id: FunctionId::HybridScore,
            name: "hybrid_score",
            arity: FunctionArity::Exact(2),
        },
        ScalarFunction {
            id: FunctionId::Snippet,
            name: "snippet",
            arity: FunctionArity::Exact(2),
        },
        ScalarFunction {
            id: FunctionId::Version,
            name: "version",
            arity: FunctionArity::Exact(0),
        },
        ScalarFunction {
            id: FunctionId::CurrentSchema,
            name: "current_schema",
            arity: FunctionArity::Exact(0),
        },
        ScalarFunction {
            id: FunctionId::CurrentDatabase,
            name: "current_database",
            arity: FunctionArity::Exact(0),
        },
        ScalarFunction {
            id: FunctionId::CurrentUser,
            name: "current_user",
            arity: FunctionArity::Exact(0),
        },
        ScalarFunction {
            id: FunctionId::SessionUser,
            name: "session_user",
            arity: FunctionArity::Exact(0),
        },
        ScalarFunction {
            id: FunctionId::CurrentRole,
            name: "current_role",
            arity: FunctionArity::Exact(0),
        },
        ScalarFunction {
            id: FunctionId::Length,
            name: "length",
            arity: FunctionArity::Exact(1),
        },
        ScalarFunction {
            id: FunctionId::Length,
            name: "len",
            arity: FunctionArity::Exact(1),
        },
        ScalarFunction {
            id: FunctionId::Lower,
            name: "lower",
            arity: FunctionArity::Exact(1),
        },
        ScalarFunction {
            id: FunctionId::Upper,
            name: "upper",
            arity: FunctionArity::Exact(1),
        },
        ScalarFunction {
            id: FunctionId::Substring,
            name: "substring",
            arity: FunctionArity::Range { min: 2, max: 3 },
        },
        ScalarFunction {
            id: FunctionId::Trim,
            name: "trim",
            arity: FunctionArity::Exact(1),
        },
        ScalarFunction {
            id: FunctionId::Concat,
            name: "concat",
            arity: FunctionArity::AtLeast(1),
        },
        ScalarFunction {
            id: FunctionId::Coalesce,
            name: "coalesce",
            arity: FunctionArity::AtLeast(1),
        },
        ScalarFunction {
            id: FunctionId::Abs,
            name: "abs",
            arity: FunctionArity::Exact(1),
        },
    ]
}
