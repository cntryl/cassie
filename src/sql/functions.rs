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
}

#[derive(Debug, Clone)]
pub struct ScalarFunction {
    pub id: FunctionId,
    pub name: &'static str,
    pub arity: usize,
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
            arity: 2,
        },
        ScalarFunction {
            id: FunctionId::SearchScore,
            name: "search_score",
            arity: 2,
        },
        ScalarFunction {
            id: FunctionId::VectorDistance,
            name: "vector_distance",
            arity: 2,
        },
        ScalarFunction {
            id: FunctionId::VectorScore,
            name: "vector_score",
            arity: 2,
        },
        ScalarFunction {
            id: FunctionId::CosineDistance,
            name: "cosine_distance",
            arity: 2,
        },
        ScalarFunction {
            id: FunctionId::DotProduct,
            name: "dot_product",
            arity: 2,
        },
        ScalarFunction {
            id: FunctionId::HybridScore,
            name: "hybrid_score",
            arity: 2,
        },
        ScalarFunction {
            id: FunctionId::Snippet,
            name: "snippet",
            arity: 2,
        },
    ]
}
