//! Actual returned rows alone reach independent and correlated EXISTS scalars.
use crate::support_returning_phases::{assert_original_rows, assert_result, seeded_fixture};
use cassie::app::CassieError;

#[test]
fn should_skip_dead_returning_case_branches_in_all_three_forms() {
    // Arrange
    let fixture = seeded_fixture();
    let expressions = [
        ("CASE WHEN id<0 THEN 1/0 ELSE 1 END", false),
        ("CASE WHEN id<0 THEN EXISTS(SELECT 1 FROM ri WHERE 1/0=0) ELSE false END", true),
        ("CASE WHEN id<0 THEN EXISTS(SELECT 1 FROM ri i WHERE i.id=rt.id AND 1/0=0) ELSE false END", true),
    ];

    for (expression, boolean) in expressions {
        // Act
        let actual = fixture.cassie.execute_sql(
            &fixture.session,
            &format!("UPDATE rt SET id=id RETURNING id, {expression} AS selected"),
            vec![],
        );

        // Assert
        assert_result(
            actual.expect("dead branch stays unevaluated"),
            boolean,
            false,
        );
        assert_original_rows(&fixture);
    }
}

#[test]
fn should_abort_changed_rows_when_returning_case_reaches_each_error_form() {
    // Arrange
    let fixture = seeded_fixture();
    let expressions = [
        "CASE WHEN id>0 THEN 1/0 ELSE 1 END",
        "CASE WHEN id>0 THEN EXISTS(SELECT 1 FROM ri WHERE 1/0=0) ELSE false END",
        "CASE WHEN id>0 THEN EXISTS(SELECT 1 FROM ri i WHERE i.id=rt.id-10 AND 1/0=0) ELSE false END",
    ];

    for expression in expressions {
        // Act
        let actual = fixture.cassie.execute_sql(
            &fixture.session,
            &format!("UPDATE rt SET id=id+10 RETURNING id, {expression} AS selected"),
            vec![],
        );

        // Assert
        assert!(
            matches!(actual, Err(CassieError::Execution(ref message)) if message == "division by zero"),
            "exact reached scalar error"
        );
        assert_original_rows(&fixture);
    }
}

#[test]
fn should_keep_static_descriptors_without_evaluating_zero_row_returning_scalars() {
    // Arrange
    let fixture = seeded_fixture();
    let expressions = [
        ("1/0", false),
        ("EXISTS(SELECT 1 FROM ri WHERE 1/0=0)", true),
        (
            "EXISTS(SELECT 1 FROM ri i WHERE i.id=rt.id AND 1/0=0)",
            true,
        ),
    ];

    for (expression, boolean) in expressions {
        // Act
        let actual = fixture.cassie.execute_sql(
            &fixture.session,
            &format!("UPDATE rt SET id=id+10 WHERE id<0 RETURNING {expression} AS selected"),
            vec![],
        );

        // Assert
        assert_result(
            actual.expect("no returned row reaches scalar"),
            boolean,
            true,
        );
        assert_original_rows(&fixture);
    }
}

#[test]
fn should_abort_changed_rows_when_null_coalesce_reaches_independent_exists_error() {
    // Arrange
    let fixture = seeded_fixture();

    // Act
    let actual = fixture.cassie.execute_sql(
        &fixture.session,
        "UPDATE rt SET id=id+10 RETURNING COALESCE(NULL, EXISTS(SELECT 1 FROM ri WHERE 1/0=0)) AS selected",
        vec![],
    );

    // Assert
    assert!(
        matches!(actual, Err(CassieError::Execution(ref message)) if message == "division by zero"),
        "exact reached COALESCE error"
    );
    assert_original_rows(&fixture);
}
