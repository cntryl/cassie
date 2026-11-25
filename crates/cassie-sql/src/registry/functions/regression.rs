//! PostgreSQL regression aggregate identities and signatures.
use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};

const fn aggregate(
    id: FunctionId,
    name: &'static str,
    return_type: FunctionReturnType,
) -> ScalarFunction {
    ScalarFunction {
        id,
        name,
        arity: FunctionArity::Exact(2),
        return_type,
        is_aggregate: true,
        bypass_arity_validation: false,
    }
}

pub const FUNCTIONS: &[ScalarFunction] = &[
    aggregate(FunctionId::Corr, "corr", FunctionReturnType::Float),
    aggregate(
        FunctionId::Corr,
        "pg_catalog.corr",
        FunctionReturnType::Float,
    ),
    aggregate(FunctionId::CovarPop, "covar_pop", FunctionReturnType::Float),
    aggregate(
        FunctionId::CovarPop,
        "pg_catalog.covar_pop",
        FunctionReturnType::Float,
    ),
    aggregate(
        FunctionId::CovarSamp,
        "covar_samp",
        FunctionReturnType::Float,
    ),
    aggregate(
        FunctionId::CovarSamp,
        "pg_catalog.covar_samp",
        FunctionReturnType::Float,
    ),
    aggregate(
        FunctionId::RegrCount,
        "regr_count",
        FunctionReturnType::BigInt,
    ),
    aggregate(
        FunctionId::RegrCount,
        "pg_catalog.regr_count",
        FunctionReturnType::BigInt,
    ),
    aggregate(
        FunctionId::RegrSlope,
        "regr_slope",
        FunctionReturnType::Float,
    ),
    aggregate(
        FunctionId::RegrSlope,
        "pg_catalog.regr_slope",
        FunctionReturnType::Float,
    ),
    aggregate(
        FunctionId::RegrIntercept,
        "regr_intercept",
        FunctionReturnType::Float,
    ),
    aggregate(
        FunctionId::RegrIntercept,
        "pg_catalog.regr_intercept",
        FunctionReturnType::Float,
    ),
    aggregate(FunctionId::RegrR2, "regr_r2", FunctionReturnType::Float),
    aggregate(
        FunctionId::RegrR2,
        "pg_catalog.regr_r2",
        FunctionReturnType::Float,
    ),
    aggregate(FunctionId::RegrSxx, "regr_sxx", FunctionReturnType::Float),
    aggregate(
        FunctionId::RegrSxx,
        "pg_catalog.regr_sxx",
        FunctionReturnType::Float,
    ),
    aggregate(FunctionId::RegrSxy, "regr_sxy", FunctionReturnType::Float),
    aggregate(
        FunctionId::RegrSxy,
        "pg_catalog.regr_sxy",
        FunctionReturnType::Float,
    ),
    aggregate(FunctionId::RegrSyy, "regr_syy", FunctionReturnType::Float),
    aggregate(
        FunctionId::RegrSyy,
        "pg_catalog.regr_syy",
        FunctionReturnType::Float,
    ),
    aggregate(FunctionId::RegrAvgx, "regr_avgx", FunctionReturnType::Float),
    aggregate(
        FunctionId::RegrAvgx,
        "pg_catalog.regr_avgx",
        FunctionReturnType::Float,
    ),
    aggregate(FunctionId::RegrAvgy, "regr_avgy", FunctionReturnType::Float),
    aggregate(
        FunctionId::RegrAvgy,
        "pg_catalog.regr_avgy",
        FunctionReturnType::Float,
    ),
];

#[must_use]
pub const fn is_regression_function(id: FunctionId) -> bool {
    matches!(
        id,
        FunctionId::Corr
            | FunctionId::CovarPop
            | FunctionId::CovarSamp
            | FunctionId::RegrCount
            | FunctionId::RegrSlope
            | FunctionId::RegrIntercept
            | FunctionId::RegrR2
            | FunctionId::RegrSxx
            | FunctionId::RegrSxy
            | FunctionId::RegrSyy
            | FunctionId::RegrAvgx
            | FunctionId::RegrAvgy
    )
}
