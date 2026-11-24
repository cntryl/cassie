use super::{constraints, version, DefinitionError};
use crate::declarations::{ConstraintCheck, ConstraintOperator, FieldConstraint};

#[test]
fn should_reject_unknown_persisted_definition_versions_before_payload_checks() {
    // Arrange
    let constraint = FieldConstraint {
        definition_semantics_version: 9,
        check: Some(ConstraintCheck {
            field: "n".to_owned(),
            operator: ConstraintOperator::IsNotDistinctFrom,
            value: 7.into(),
            bound_expression: None,
        }),
        ..FieldConstraint::<()>::new("n")
    };

    // Act
    let error = constraints(&[constraint]).expect_err("unknown definition version");

    // Assert
    assert!(matches!(error, DefinitionError::Unsupported(_)));
    assert_eq!(
        error.to_string(),
        "unsupported feature: unknown persisted definition semantics version 9"
    );
    assert!(version(0).is_ok());
    assert!(version(1).is_ok());
}
