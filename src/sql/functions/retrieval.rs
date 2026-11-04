//! Text, vector and hybrid retrieval function registry metadata.

use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};

pub(super) const FUNCTIONS: &[ScalarFunction] = &[
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
];
