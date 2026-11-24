//! Engine diagnostics for the persisted CHECK shape.

use crate::app::CassieError;
use crate::catalog::ConstraintCheck;
pub(crate) use cassie_sql::definition_guard::{exact_eq, field_type, literal};

pub(crate) fn validate(check: &ConstraintCheck) -> Result<(), CassieError> {
    Ok(cassie_sql::definition_guard::validate(check)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::ConstraintOperator;
    use crate::sql::ast::Expr;
    use crate::types::DataType;
    #[test]
    fn should_reject_shapes_outside_the_prepared_simple_comparison() {
        // Arrange
        let cases = [
            ("CAST(n AS BIGINT) IS NOT DISTINCT FROM 7", true),
            ("CAST(m AS BIGINT) IS NOT DISTINCT FROM 7", false),
            ("CAST(n AS BIGINT) IS NOT DISTINCT FROM 8", false),
            ("CAST(n AS BIGINT) IS DISTINCT FROM 7", false),
            ("ABS(n) IS NOT DISTINCT FROM 7", false),
            ("CAST(n+1 AS BIGINT) IS NOT DISTINCT FROM 7", false),
            ("CAST(n AS BIGINT) IS NOT DISTINCT FROM ABS(7)", false),
        ];
        for (sql, accepted) in cases {
            let check = ConstraintCheck {
                field: "n".into(),
                operator: ConstraintOperator::IsNotDistinctFrom,
                value: 7.into(),
                bound_expression: Some(Box::new(
                    crate::sql::parser::parse_expression(sql).expect("expression"),
                )),
            };
            // Act
            let result = validate(&check);
            // Assert
            assert_eq!(result.is_ok(), accepted, "{sql}: {result:?}");
        }
    }
    #[test]
    fn should_distinguish_cast_provenance_in_check_metadata_identity() {
        // Arrange
        let left = crate::sql::parser::parse_expression("CAST(n AS BIGINT) IS NOT DISTINCT FROM 7")
            .expect("simple check");
        let mut right = left.clone();
        let Expr::Binary { left: operand, .. } = &mut right else {
            panic!("comparison");
        };
        let Expr::Cast { source_type, .. } = operand.as_mut() else {
            panic!("cast");
        };
        *source_type = Some(DataType::Float);
        // Act
        let unchanged = exact_eq(&left, &left.clone());
        let altered = exact_eq(&left, &right);
        // Assert
        assert!(unchanged);
        assert!(!altered);
    }
}
