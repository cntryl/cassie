pub use cassie_core::catalog::declarations::{IdentityGeneration, UniqueNullPolicy};
pub use cassie_sql::declarations::{
    generated_constraint_name, merge_constraint_set, ConstraintOperator,
    DefaultSequenceOwnership, NotNullOwnership,
};
pub type ConstraintCheck = cassie_sql::declarations::ConstraintCheck<crate::sql::SubqueryOutcome>;
pub type FieldConstraint = cassie_sql::declarations::FieldConstraint<crate::sql::SubqueryOutcome>;

/// Rebinds storage-decoded constraint syntax into the engine attachment type.
pub(crate) fn attach_storage_constraints(
    constraints: Vec<cassie_sql::declarations::FieldConstraint<()>>,
) -> Vec<FieldConstraint> {
    use cassie_sql::attachment::MapAttachment;

    constraints.rebind_attachment::<crate::sql::SubqueryOutcome>()
}

#[cfg(test)]
mod tests {
    use super::{attach_storage_constraints, DefaultSequenceOwnership, FieldConstraint};

    #[test]
    fn should_serialize_default_sequence_ownership_as_bool() {
        // Arrange
        let mut constraint = FieldConstraint::new("id");
        constraint.default_sequence_owned = DefaultSequenceOwnership::owned();

        // Act
        let json = serde_json::to_value(&constraint).expect("serialize field constraint");

        // Assert
        assert_eq!(
            json["default_sequence_owned"],
            serde_json::Value::Bool(true)
        );
    }

    #[test]
    fn should_rebind_storage_constraint_expression_into_engine_attachment_type() {
        // Arrange
        let mut constraint = cassie_sql::declarations::FieldConstraint::<()>::new("value");
        constraint.check = Some(cassie_sql::declarations::ConstraintCheck {
            field: "value".to_string(),
            operator: cassie_sql::declarations::ConstraintOperator::Eq,
            value: serde_json::Value::from(1),
            bound_expression: Some(Box::new(cassie_sql::ast::Expr::IntegerLiteral(1))),
        });
        let serialized = serde_json::to_vec(&constraint).expect("serialize stored constraint");
        let decoded: cassie_sql::declarations::FieldConstraint<()> =
            serde_json::from_slice(&serialized).expect("decode stored constraint");

        // Act
        let rebound = attach_storage_constraints(vec![decoded]);

        // Assert
        assert!(matches!(
            rebound[0]
                .check
                .as_ref()
                .and_then(|check| check.bound_expression.as_deref()),
            Some(cassie_sql::ast::Expr::IntegerLiteral(1))
        ));
    }
}
