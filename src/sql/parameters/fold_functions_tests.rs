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
fn should_exclude_resolved_and_shared_count_slots_from_unknown_bucket_inference() {
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
    assert!(all_unknown_bucket_parameters(&distinct, &unknowns));
    assert!(all_unknown_bucket_parameters(&repeated_operands, &unknowns));
    assert!(!all_unknown_bucket_parameters(&shared_count, &unknowns));
    assert!(!all_unknown_bucket_parameters(&distinct, &resolved));
}
