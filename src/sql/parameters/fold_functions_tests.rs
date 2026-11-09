use super::{all_unknown_bucket_parameters, ast, Summary};

fn unknown() -> Summary {
    Summary {
        oid: 705,
        occurrence: None,
        qualified: true,
        literal: false,
    }
}

#[test]
fn should_only_admit_fresh_shared_count_slots_into_unknown_bucket_inference() {
    // Arrange
    let distinct = [
        ast::Expr::Param(0),
        ast::Expr::Param(1),
        ast::Expr::Param(2),
        ast::Expr::Param(3),
    ];
    let repeated_operands = [
        ast::Expr::Param(0),
        ast::Expr::Param(0),
        ast::Expr::Param(2),
        ast::Expr::Param(3),
    ];
    let shared_count = [
        ast::Expr::Param(0),
        ast::Expr::Param(1),
        ast::Expr::Param(2),
        ast::Expr::Param(0),
    ];
    let unknowns = [unknown(); 4];
    let mut resolved = unknowns;
    resolved[0] = Summary::known(25);

    // Act / Assert
    assert!(all_unknown_bucket_parameters(&distinct, &unknowns, false));
    assert!(all_unknown_bucket_parameters(
        &repeated_operands,
        &unknowns,
        false
    ));
    assert!(!all_unknown_bucket_parameters(
        &shared_count,
        &unknowns,
        false
    ));
    assert!(all_unknown_bucket_parameters(
        &shared_count,
        &unknowns,
        true
    ));
    let mut contextual_null = unknowns;
    contextual_null[0].literal = true;
    assert!(!all_unknown_bucket_parameters(
        &shared_count,
        &contextual_null,
        true
    ));
    assert!(!all_unknown_bucket_parameters(&distinct, &resolved, false));
}

#[test]
fn should_detect_an_earlier_unqualified_parameter_demand() {
    // Arrange
    let controls = crate::runtime::QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    let mut occurrences = super::super::occurrences::Occurrences::new(
        1,
        super::super::occurrences::PrivateParameterTypeInput::Wire(&[]),
        &controls,
        true,
    )
    .expect("occurrence ledger");
    let mut prior = occurrences
        .parameter(0, &controls, false)
        .expect("prior occurrence");

    // Act
    occurrences
        .coerce(&mut prior, 701, true)
        .expect("legacy demand");

    // Assert
    assert!(occurrences.has_legacy_inference(&prior));
}

#[test]
fn should_report_a_fresh_shared_parameter_as_an_inconsistent_type() {
    // Arrange
    let controls = crate::runtime::QueryExecutionControls::from_limits(
        &crate::config::CassieRuntimeLimits::default(),
        std::time::Instant::now(),
    );
    let mut occurrences = super::super::occurrences::Occurrences::new(
        1,
        super::super::occurrences::PrivateParameterTypeInput::Wire(&[]),
        &controls,
        true,
    )
    .expect("occurrence ledger");
    let mut operand = occurrences.parameter(0, &controls, true).expect("operand");
    let mut count = occurrences.parameter(0, &controls, true).expect("count");

    // Act
    occurrences
        .coerce(&mut operand, 701, true)
        .expect("operand type");
    let result = occurrences.coerce(&mut count, 23, true);

    // Assert
    assert!(matches!(
        result,
        Err(super::super::ParameterAdmissionError::Semantic(
            super::super::diagnostics::ParameterSemanticDiagnostic::Inconsistent {
                slot: 0,
                previous: 701,
                requested: 23,
            }
        ))
    ));
}
