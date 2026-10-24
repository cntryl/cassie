//! PostgreSQL's arbitrary non-NULL aggregate.

use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};

pub(super) const FUNCTIONS: &[ScalarFunction] = &[ScalarFunction {
    id: FunctionId::AnyValue,
    name: "any_value",
    arity: FunctionArity::Exact(1),
    return_type: FunctionReturnType::FirstNonNullArgument,
    is_aggregate: true,
    bypass_arity_validation: false,
}];
