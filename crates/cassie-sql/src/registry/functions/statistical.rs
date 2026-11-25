//! Statistical aggregate identities and their PostgreSQL names.
use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};

const fn aggregate(id: FunctionId, name: &'static str) -> ScalarFunction {
    ScalarFunction {
        id,
        name,
        arity: FunctionArity::Exact(1),
        return_type: FunctionReturnType::Unknown,
        is_aggregate: true,
        bypass_arity_validation: false,
    }
}

pub const FUNCTIONS: &[ScalarFunction] = &[
    aggregate(FunctionId::VarPop, "var_pop"),
    aggregate(FunctionId::VarPop, "pg_catalog.var_pop"),
    aggregate(FunctionId::VarSamp, "var_samp"),
    aggregate(FunctionId::VarSamp, "pg_catalog.var_samp"),
    aggregate(FunctionId::Variance, "variance"),
    aggregate(FunctionId::Variance, "pg_catalog.variance"),
    aggregate(FunctionId::StddevPop, "stddev_pop"),
    aggregate(FunctionId::StddevPop, "pg_catalog.stddev_pop"),
    aggregate(FunctionId::StddevSamp, "stddev_samp"),
    aggregate(FunctionId::StddevSamp, "pg_catalog.stddev_samp"),
    aggregate(FunctionId::Stddev, "stddev"),
    aggregate(FunctionId::Stddev, "pg_catalog.stddev"),
];

#[must_use]
pub const fn is_statistical_function(id: FunctionId) -> bool {
    matches!(
        id,
        FunctionId::VarPop
            | FunctionId::VarSamp
            | FunctionId::Variance
            | FunctionId::StddevPop
            | FunctionId::StddevSamp
            | FunctionId::Stddev
    )
}
