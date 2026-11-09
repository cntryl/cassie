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

#[test]
fn should_restrict_mixed_bucket_inference_to_fresh_uniform_anchors() {
    // Arrange
    let args = [
        ast::Expr::Param(0),
        ast::Expr::IntegerLiteral(0),
        ast::Expr::IntegerLiteral(1),
        ast::Expr::IntegerLiteral(2),
    ];
    let summaries = [
        unknown(),
        Summary::known(701),
        Summary::known(701),
        Summary::known(23),
    ];
    let fresh = [true, false, false, false];
    let mut mixed = summaries;
    mixed[2] = Summary::known(1700);
    let mut prior_resolved = summaries;
    prior_resolved[0] = Summary::known(23);
    let mut unqualified = summaries;
    unqualified[0].qualified = false;
    let mut contextual_null = summaries;
    contextual_null[0].literal = true;
    let supplied_anchor = [
        ast::Expr::Param(0),
        ast::Expr::Param(1),
        ast::Expr::IntegerLiteral(1),
        ast::Expr::IntegerLiteral(2),
    ];
    let same_slot = [
        ast::Expr::Param(0),
        ast::Expr::IntegerLiteral(0),
        ast::Expr::IntegerLiteral(1),
        ast::Expr::Param(0),
    ];
    let unknown_count = [
        unknown(),
        Summary::known(701),
        Summary::known(701),
        unknown(),
    ];
    let distinct_count = [
        ast::Expr::Param(0),
        ast::Expr::IntegerLiteral(0),
        ast::Expr::IntegerLiteral(1),
        ast::Expr::Param(1),
    ];
    let mut small_count = summaries;
    small_count[3] = Summary::known(21);
    let no_anchor_args = [
        ast::Expr::Param(0),
        ast::Expr::Param(1),
        ast::Expr::Param(2),
        ast::Expr::IntegerLiteral(2),
    ];
    let no_anchor = [unknown(), unknown(), unknown(), Summary::known(23)];
    let mut no_anchor_small = no_anchor;
    no_anchor_small[3] = Summary::known(21);
    let mut no_anchor_prior = no_anchor;
    no_anchor_prior[0] = Summary::known(23);

    // Act / Assert
    assert_eq!(
        super::mixed_bucket_targets(&args, &summaries, &fresh),
        Some([701, 701, 701, 23])
    );
    assert_eq!(super::mixed_bucket_targets(&args, &mixed, &fresh), None);
    assert_eq!(
        super::mixed_bucket_targets(&args, &prior_resolved, &fresh),
        None
    );
    assert_eq!(
        super::mixed_bucket_targets(&args, &unqualified, &fresh),
        None
    );
    assert_eq!(
        super::mixed_bucket_targets(&args, &contextual_null, &fresh),
        None
    );
    assert_eq!(
        super::mixed_bucket_targets(&args, &summaries, &[false; 4]),
        None
    );
    assert_eq!(
        super::mixed_bucket_targets(&supplied_anchor, &summaries, &fresh),
        None
    );
    assert_eq!(
        super::mixed_bucket_targets(&same_slot, &unknown_count, &[true, false, false, true]),
        None
    );
    assert_eq!(
        super::mixed_bucket_targets(&distinct_count, &unknown_count, &[true, false, false, true]),
        Some([701, 701, 701, 23])
    );
    assert_eq!(
        super::mixed_bucket_targets(&args, &small_count, &fresh),
        None
    );
    assert_eq!(
        super::mixed_bucket_targets(&no_anchor_args, &no_anchor, &[true, true, true, false]),
        Some([701, 701, 701, 23])
    );
    assert_eq!(
        super::mixed_bucket_targets(
            &no_anchor_args,
            &no_anchor_small,
            &[true, true, true, false]
        ),
        None
    );
    assert_eq!(
        super::mixed_bucket_targets(
            &no_anchor_args,
            &no_anchor_prior,
            &[true, true, true, false]
        ),
        None
    );
}

#[test]
fn should_preserve_a_prior_count_cast_when_an_operand_snapshot_was_unknown() {
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
    let mut count = occurrences
        .parameter(0, &controls, true)
        .expect("count cast");
    occurrences
        .coerce(&mut count, 23, true)
        .expect("count cast type");

    // Act
    let result = occurrences.coerce(&mut operand, 701, true);

    // Assert
    assert!(matches!(
        result,
        Err(super::super::ParameterAdmissionError::Semantic(
            super::super::diagnostics::ParameterSemanticDiagnostic::Inconsistent {
                slot: 0,
                previous: 23,
                requested: 701,
            }
        ))
    ));
}
