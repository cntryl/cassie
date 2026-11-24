pub use cassie_core::catalog::declarations::{IdentityGeneration, UniqueNullPolicy};
pub use cassie_sql::declarations::{
    generated_constraint_name, merge_constraint_set, ConstraintOperator,
    DefaultSequenceOwnership, NotNullOwnership,
};
pub type ConstraintCheck = cassie_sql::declarations::ConstraintCheck<crate::sql::SubqueryOutcome>;
pub type FieldConstraint = cassie_sql::declarations::FieldConstraint<crate::sql::SubqueryOutcome>;

#[cfg(test)]
mod tests {
    use super::{DefaultSequenceOwnership, FieldConstraint};

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
}
