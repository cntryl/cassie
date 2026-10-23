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

pub(crate) const FUNCTIONS: &[ScalarFunction] = &[
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

pub(crate) const fn is_regression_function(id: FunctionId) -> bool {
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

#[cfg(test)]
mod tests {
    use super::{is_regression_function, FunctionId};
    use crate::sql::functions::{
        function, input_signature, FunctionArity, FunctionInputSignature, FunctionReturnType,
    };

    #[test]
    fn should_register_two_float8_regression_aggregate_signatures() {
        // Arrange
        let identities = [
            ("corr", FunctionId::Corr),
            ("covar_pop", FunctionId::CovarPop),
            ("covar_samp", FunctionId::CovarSamp),
            ("regr_count", FunctionId::RegrCount),
            ("regr_slope", FunctionId::RegrSlope),
            ("regr_intercept", FunctionId::RegrIntercept),
            ("regr_r2", FunctionId::RegrR2),
            ("regr_sxx", FunctionId::RegrSxx),
            ("regr_sxy", FunctionId::RegrSxy),
            ("regr_syy", FunctionId::RegrSyy),
            ("regr_avgx", FunctionId::RegrAvgx),
            ("regr_avgy", FunctionId::RegrAvgy),
        ];

        // Act / Assert
        for (name, expected) in identities {
            let registered = function(name).expect("registered regression aggregate");
            assert!(registered.is_aggregate);
            assert_eq!(registered.id, expected);
            assert_eq!(registered.arity, FunctionArity::Exact(2));
            assert!(is_regression_function(registered.id));
            let expected_return = if expected == FunctionId::RegrCount {
                FunctionReturnType::BigInt
            } else {
                FunctionReturnType::Float
            };
            assert_eq!(registered.return_type, expected_return);
            let qualified = format!("pg_catalog.{name}");
            assert_eq!(
                function(&qualified).expect("qualified aggregate").id,
                expected
            );
            assert!(matches!(
                input_signature(registered.id),
                Some(FunctionInputSignature::Positional(signature)) if signature == [701, 701]
            ));
        }
    }
}
