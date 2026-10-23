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

pub(crate) const FUNCTIONS: &[ScalarFunction] = &[
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

pub(crate) const fn is_statistical_function(id: FunctionId) -> bool {
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

#[cfg(test)]
mod tests {
    use super::{is_statistical_function, FunctionId};
    use crate::sql::functions::{function, FunctionInputSignature};

    #[test]
    fn should_register_named_statistical_aggregate_identities() {
        // Arrange
        let identities = [
            ("var_pop", FunctionId::VarPop),
            ("var_samp", FunctionId::VarSamp),
            ("variance", FunctionId::Variance),
            ("stddev_pop", FunctionId::StddevPop),
            ("stddev_samp", FunctionId::StddevSamp),
            ("stddev", FunctionId::Stddev),
        ];

        // Act
        for (name, expected) in identities {
            let registered = function(name).expect("registered statistical aggregate");
            // Assert
            assert!(registered.is_aggregate);
            assert_eq!(registered.id, expected);
            assert!(is_statistical_function(registered.id));
            assert!(matches!(
                crate::sql::functions::input_signature(registered.id),
                Some(FunctionInputSignature::CommonArguments)
            ));
        }
    }
}
