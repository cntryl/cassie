//! Ordered-set and hypothetical-set aggregate identities.

use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};

pub(super) const FUNCTIONS: &[ScalarFunction] = &[
    aggregate(
        FunctionId::Mode,
        "mode",
        FunctionArity::Exact(0),
        FunctionReturnType::Unknown,
    ),
    aggregate(
        FunctionId::Mode,
        "pg_catalog.mode",
        FunctionArity::Exact(0),
        FunctionReturnType::Unknown,
    ),
    aggregate(
        FunctionId::PercentileCont,
        "percentile_cont",
        FunctionArity::Exact(1),
        FunctionReturnType::Unknown,
    ),
    aggregate(
        FunctionId::PercentileCont,
        "pg_catalog.percentile_cont",
        FunctionArity::Exact(1),
        FunctionReturnType::Unknown,
    ),
    aggregate(
        FunctionId::PercentileDisc,
        "percentile_disc",
        FunctionArity::Exact(1),
        FunctionReturnType::Unknown,
    ),
    aggregate(
        FunctionId::PercentileDisc,
        "pg_catalog.percentile_disc",
        FunctionArity::Exact(1),
        FunctionReturnType::Unknown,
    ),
    aggregate(
        FunctionId::Rank,
        "rank",
        FunctionArity::AtLeast(1),
        FunctionReturnType::BigInt,
    ),
    aggregate(
        FunctionId::Rank,
        "pg_catalog.rank",
        FunctionArity::AtLeast(1),
        FunctionReturnType::BigInt,
    ),
    aggregate(
        FunctionId::DenseRank,
        "dense_rank",
        FunctionArity::AtLeast(1),
        FunctionReturnType::BigInt,
    ),
    aggregate(
        FunctionId::DenseRank,
        "pg_catalog.dense_rank",
        FunctionArity::AtLeast(1),
        FunctionReturnType::BigInt,
    ),
    aggregate(
        FunctionId::PercentRank,
        "percent_rank",
        FunctionArity::AtLeast(1),
        FunctionReturnType::Float,
    ),
    aggregate(
        FunctionId::PercentRank,
        "pg_catalog.percent_rank",
        FunctionArity::AtLeast(1),
        FunctionReturnType::Float,
    ),
    aggregate(
        FunctionId::CumeDist,
        "cume_dist",
        FunctionArity::AtLeast(1),
        FunctionReturnType::Float,
    ),
    aggregate(
        FunctionId::CumeDist,
        "pg_catalog.cume_dist",
        FunctionArity::AtLeast(1),
        FunctionReturnType::Float,
    ),
];

const fn aggregate(
    id: FunctionId,
    name: &'static str,
    arity: FunctionArity,
    return_type: FunctionReturnType,
) -> ScalarFunction {
    ScalarFunction {
        id,
        name,
        arity,
        return_type,
        is_aggregate: true,
        bypass_arity_validation: false,
    }
}
