//! Scalar catalog helpers using existing primitive input and result types.
use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};
pub(super) const FUNCTIONS: &[ScalarFunction] = &[
    ScalarFunction {
        id: FunctionId::PgCharToEncoding,
        name: "pg_char_to_encoding",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Int,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgSizeBytes,
        name: "pg_size_bytes",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::BigInt,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
    ScalarFunction {
        id: FunctionId::PgSizePretty,
        name: "pg_size_pretty",
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Text,
        is_aggregate: false,
        bypass_arity_validation: false,
    },
];
