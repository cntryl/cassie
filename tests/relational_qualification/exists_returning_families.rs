//! Separate ordinary/EXISTS family witnesses; new actual qualification is pending.
use crate::support_returning_families::{
    assert_fresh_rows, assert_result, ids, inserted_rows, matched_rows, seeded, sql,
};
use cassie::app::CassieError;

const COLUMNS: &[(&str, bool)] = &[
    ("id", false),
    ("any_inner", true),
    ("correlated", true),
    ("self_member", true),
];
const INSERT_COLUMNS: &[(&str, bool)] = &[
    ("id", false),
    ("any_inner", true),
    ("correlated", true),
    ("self_member", true),
    ("prior_write", true),
];

#[test]
fn should_resolve_insert_values_returning_with_prior_overlay() {
    // Arrange
    let fixture = seeded();
    sql(&fixture, "BEGIN");
    sql(&fixture, "INSERT INTO family_target VALUES(9)");

    // Act
    let ordinary = sql(
        &fixture,
        "INSERT INTO family_control VALUES(1),(2) RETURNING id",
    );
    let selected = sql(
        &fixture,
        "INSERT INTO family_target VALUES(1),(2) RETURNING id, \
         EXISTS(SELECT 1 FROM family_inner) AS any_inner, \
         EXISTS(SELECT 1 FROM family_inner i WHERE i.id=family_target.id) AS correlated, \
         EXISTS(SELECT 1 FROM family_target t WHERE t.id=family_target.id) AS self_member, \
         EXISTS(SELECT 1 FROM family_target WHERE id=9) AS prior_write",
    );
    let later = sql(&fixture, "SELECT id FROM family_target ORDER BY id");

    // Assert
    assert_result(ordinary, "INSERT 0 2", &[("id", false)], &ids(&[1, 2]));
    assert_result(selected, "INSERT 0 2", INSERT_COLUMNS, &inserted_rows());
    assert_result(later, "SELECT", &[("id", false)], &ids(&[1, 2, 9]));
    assert_fresh_rows(&fixture, "family_target", &[("id", false)], &[]);
    sql(&fixture, "COMMIT");
    assert_fresh_rows(
        &fixture,
        "family_target",
        &[("id", false)],
        &ids(&[1, 2, 9]),
    );
}

#[test]
fn should_resolve_insert_select_returning_with_literal_family_descriptors() {
    // Arrange
    let fixture = seeded();
    sql(&fixture, "BEGIN");
    sql(&fixture, "INSERT INTO family_target VALUES(9)");

    // Act
    let ordinary = sql(
        &fixture,
        "INSERT INTO family_control SELECT id FROM family_inner RETURNING id",
    );
    sql(&fixture, "INSERT INTO family_control VALUES(2)");
    let selected = sql(
        &fixture,
        "INSERT INTO family_target SELECT id FROM family_control ORDER BY id RETURNING id, \
         EXISTS(SELECT 1 FROM family_inner) AS any_inner, \
         EXISTS(SELECT 1 FROM family_inner i WHERE i.id=family_target.id) AS correlated, \
         EXISTS(SELECT 1 FROM family_target t WHERE t.id=family_target.id) AS self_member, \
         EXISTS(SELECT 1 FROM family_target WHERE id=9) AS prior_write",
    );
    let later = sql(&fixture, "SELECT id FROM family_target ORDER BY id");

    // Assert
    assert_result(ordinary, "INSERT 0 1", &[("id", false)], &ids(&[1]));
    assert_result(selected, "INSERT 0 2", INSERT_COLUMNS, &inserted_rows());
    assert_result(later, "SELECT", &[("id", false)], &ids(&[1, 2, 9]));
    assert_fresh_rows(&fixture, "family_target", &[("id", false)], &[]);
    sql(&fixture, "COMMIT");
    assert_fresh_rows(
        &fixture,
        "family_target",
        &[("id", false)],
        &ids(&[1, 2, 9]),
    );
}

#[test]
fn should_resolve_changed_update_rows_with_literal_family_descriptors() {
    // Arrange
    let fixture = seeded();
    sql(&fixture, "INSERT INTO family_target VALUES(1),(2)");
    sql(&fixture, "INSERT INTO family_control VALUES(1),(2)");

    // Act
    let ordinary = sql(&fixture, "UPDATE family_control SET id=id RETURNING id");
    let selected = sql(
        &fixture,
        "UPDATE family_target SET id=id+10 RETURNING id, \
         EXISTS(SELECT 1 FROM family_inner) AS any_inner, \
         EXISTS(SELECT 1 FROM family_inner i WHERE i.id=family_target.id-10) AS correlated, \
         EXISTS(SELECT 1 FROM family_target t WHERE t.id=family_target.id-10) AS self_member",
    );

    // Assert
    assert_result(ordinary, "UPDATE 2", &[("id", false)], &ids(&[1, 2]));
    assert_result(selected, "UPDATE 2", COLUMNS, &matched_rows([11, 12]));
    assert_fresh_rows(&fixture, "family_target", &[("id", false)], &ids(&[11, 12]));
}

#[test]
fn should_resolve_deleted_old_rows_with_literal_family_descriptors() {
    // Arrange
    let fixture = seeded();
    sql(&fixture, "INSERT INTO family_target VALUES(1),(2)");
    sql(&fixture, "INSERT INTO family_control VALUES(1),(2)");

    // Act
    let ordinary = sql(&fixture, "DELETE FROM family_control RETURNING id");
    let selected = sql(
        &fixture,
        "DELETE FROM family_target RETURNING id, \
         EXISTS(SELECT 1 FROM family_inner) AS any_inner, \
         EXISTS(SELECT 1 FROM family_inner i WHERE i.id=family_target.id) AS correlated, \
         EXISTS(SELECT 1 FROM family_target t WHERE t.id=family_target.id) AS self_member",
    );

    // Assert
    assert_result(ordinary, "DELETE 2", &[("id", false)], &ids(&[1, 2]));
    assert_result(selected, "DELETE 2", COLUMNS, &matched_rows([1, 2]));
    assert_fresh_rows(&fixture, "family_target", &[("id", false)], &[]);
    assert_fresh_rows(&fixture, "family_control", &[("id", false)], &[]);
}

#[test]
fn should_abort_delete_when_returning_reaches_each_exists_error() {
    // Arrange
    let fixture = seeded();
    sql(&fixture, "INSERT INTO family_target VALUES(1),(2)");
    let expressions = [
        "EXISTS(SELECT 1 FROM family_inner WHERE 1/0=0)",
        "EXISTS(SELECT 1 FROM family_inner i WHERE i.id=family_target.id AND 1/0=0)",
    ];
    for expression in expressions {
        // Act
        let actual = fixture.cassie.execute_sql(
            &fixture.session,
            &format!("DELETE FROM family_target RETURNING {expression} AS selected"),
            vec![],
        );

        // Assert
        assert!(
            matches!(actual, Err(CassieError::Execution(ref message)) if message == "division by zero"),
            "exact reached DELETE RETURNING error: {actual:?}"
        );
        assert_fresh_rows(&fixture, "family_target", &[("id", false)], &ids(&[1, 2]));
    }
}

#[test]
fn should_check_fresh_insert_uniqueness_before_reached_returning_error() {
    // Arrange
    let fixture = seeded();
    sql(
        &fixture,
        "CREATE TABLE authority_target(id BIGINT, code BIGINT UNIQUE)",
    );
    sql(&fixture, "CREATE TABLE authority_error(n BIGINT)");
    sql(&fixture, "INSERT INTO authority_error VALUES(0)");

    // Act
    let duplicate = fixture.cassie.execute_sql(
        &fixture.session,
        "INSERT INTO authority_target VALUES(1,7),(2,7) \
         RETURNING EXISTS(SELECT e.n/e.n FROM authority_error e)",
        vec![],
    );

    // Assert
    assert!(
        matches!(duplicate, Err(CassieError::UniqueViolation { ref table, ref column, ref constraint })
            if table == "postgres.public.authority_target" && column == "code" && constraint == "authority_target_code_unique"),
        "fresh preparation must report the earlier exact UNIQUE error: {duplicate:?}"
    );
    assert_fresh_rows(
        &fixture,
        "authority_target",
        &[("id", false), ("code", false)],
        &[],
    );

    // Distinct-value reachability control.
    let distinct = fixture.cassie.execute_sql(
        &fixture.session,
        "INSERT INTO authority_target VALUES(1,7),(2,8) \
         RETURNING EXISTS(SELECT e.n/e.n FROM authority_error e)",
        vec![],
    );

    // Assert
    assert!(
        matches!(distinct, Err(CassieError::Execution(ref message)) if message == "division by zero"),
        "distinct-value control must reach RETURNING: {distinct:?}"
    );
    assert_fresh_rows(
        &fixture,
        "authority_target",
        &[("id", false), ("code", false)],
        &[],
    );
}
