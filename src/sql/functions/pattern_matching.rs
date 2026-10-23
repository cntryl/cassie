//! Registered regular-expression scalar functions with stable SQL identities.

use super::{FunctionArity, FunctionId, FunctionReturnType, ScalarFunction};

const fn scalar(
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
        is_aggregate: false,
        bypass_arity_validation: false,
    }
}

pub(super) const FUNCTIONS: &[ScalarFunction] = &[
    scalar(
        FunctionId::RegexpLike,
        "regexp_like",
        FunctionArity::Range { min: 2, max: 3 },
        FunctionReturnType::Boolean,
    ),
    scalar(
        FunctionId::RegexpLike,
        "pg_catalog.regexp_like",
        FunctionArity::Range { min: 2, max: 3 },
        FunctionReturnType::Boolean,
    ),
    scalar(
        FunctionId::RegexpCount,
        "regexp_count",
        FunctionArity::Range { min: 2, max: 4 },
        FunctionReturnType::Int,
    ),
    scalar(
        FunctionId::RegexpCount,
        "pg_catalog.regexp_count",
        FunctionArity::Range { min: 2, max: 4 },
        FunctionReturnType::Int,
    ),
    scalar(
        FunctionId::RegexpReplace,
        "regexp_replace",
        FunctionArity::Range { min: 3, max: 6 },
        FunctionReturnType::Text,
    ),
    scalar(
        FunctionId::RegexpReplace,
        "pg_catalog.regexp_replace",
        FunctionArity::Range { min: 3, max: 6 },
        FunctionReturnType::Text,
    ),
];

#[cfg(test)]
mod tests {
    use super::{FunctionArity, FunctionId, FunctionReturnType};

    #[test]
    fn should_register_regexp_replace_with_stable_text_identity() {
        // Arrange
        let names = ["regexp_replace", "pg_catalog.regexp_replace"];

        // Act
        let functions = names.map(|name| {
            crate::sql::functions::function(name).expect("regexp_replace is registered")
        });

        // Assert
        for function in functions {
            assert_eq!(function.id, FunctionId::RegexpReplace);
            assert_eq!(function.arity, FunctionArity::Range { min: 3, max: 6 });
            assert_eq!(function.return_type, FunctionReturnType::Text);
            assert!(!function.is_aggregate);
        }
    }
}
