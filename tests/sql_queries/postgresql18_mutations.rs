//! PostgreSQL 18 mutation forms tracked by #796, #823, and #825.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

#[test]
fn should_assign_multiple_update_columns_from_a_scalar_subquery() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-update-tuple-subquery",
        &[
            "CREATE TABLE update_tuple_target (id INT PRIMARY KEY, label TEXT, version INT)",
            "INSERT INTO update_tuple_target VALUES (1, 'old', 1), (2, 'keep', 2)",
        ],
    );

    // Act
    let changed = fixture.execute(
        "UPDATE update_tuple_target AS target SET (label, version) = (SELECT 'next', 9) WHERE id = 1 RETURNING target.id, target.label, target.version",
    );
    let stored = fixture.execute("SELECT id, label, version FROM update_tuple_target ORDER BY id");

    // Assert
    assert_eq!(
        changed
            .expect("tuple assignment from a scalar subquery")
            .rows(),
        &[vec![
            Value::Int64(1),
            Value::String("next".to_owned()),
            Value::Int64(9),
        ]]
    );
    assert_eq!(
        stored.expect("updated and untouched rows").rows(),
        &[
            vec![
                Value::Int64(1),
                Value::String("next".to_owned()),
                Value::Int64(9),
            ],
            vec![
                Value::Int64(2),
                Value::String("keep".to_owned()),
                Value::Int64(2),
            ],
        ]
    );
}

#[test]
fn should_insert_default_values_using_all_declared_defaults() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-insert-default-values",
        &[
            "CREATE TABLE default_insert_target (label TEXT DEFAULT 'pending', version INT DEFAULT 7)",
        ],
    );

    // Act
    let inserted = fixture
        .execute("INSERT INTO default_insert_target DEFAULT VALUES RETURNING label, version");

    // Assert
    assert_eq!(
        inserted.expect("INSERT DEFAULT VALUES").rows(),
        &[vec![Value::String("pending".to_owned()), Value::Int64(7)]]
    );
}

#[test]
fn should_apply_default_keywords_in_insert_values_rows() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-insert-default-values-list",
        &[
            "CREATE TABLE default_values_target (label TEXT DEFAULT 'pending', version INT DEFAULT 7)",
        ],
    );

    // Act
    let inserted = fixture.execute(
        "INSERT INTO default_values_target VALUES (DEFAULT, 9), ('custom', DEFAULT) RETURNING label, version",
    );

    // Assert
    assert_eq!(
        inserted
            .expect("DEFAULT values in an INSERT VALUES list")
            .rows(),
        &[
            vec![Value::String("pending".to_owned()), Value::Int64(9)],
            vec![Value::String("custom".to_owned()), Value::Int64(7)],
        ]
    );
}

#[test]
fn should_apply_default_to_an_update_assignment() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-update-default-assignment",
        &[
            "CREATE TABLE default_update_target (id INT PRIMARY KEY, label TEXT DEFAULT 'pending')",
            "INSERT INTO default_update_target VALUES (1, 'old')",
        ],
    );

    // Act
    let updated = fixture.execute(
        "UPDATE default_update_target SET label = DEFAULT WHERE id = 1 RETURNING id, label",
    );

    // Assert
    assert_eq!(
        updated.expect("DEFAULT in an UPDATE assignment").rows(),
        &[vec![Value::Int64(1), Value::String("pending".to_owned())]]
    );
}

#[test]
fn should_return_update_row_images() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-update-old-new-returning",
        &[
            "CREATE TABLE update_old_new_target (id INT PRIMARY KEY, label TEXT)",
            "INSERT INTO update_old_new_target VALUES (1, 'old')",
        ],
    );

    // Act
    let updated = fixture.execute(
        "UPDATE update_old_new_target SET label = 'new' WHERE id = 1 RETURNING WITH (OLD AS old_row, NEW AS new_row) old_row.id, old_row.label, new_row.id, new_row.label",
    );

    // Assert
    assert_eq!(
        updated.expect("UPDATE RETURNING OLD and NEW rows").rows(),
        &[vec![
            Value::Int64(1),
            Value::String("old".to_owned()),
            Value::Int64(1),
            Value::String("new".to_owned()),
        ]]
    );
}

#[test]
fn should_return_delete_row_images() {
    // Arrange
    let fixture = sql_fixture(
        "issue-823-delete-old-new-returning",
        &[
            "CREATE TABLE delete_old_new_target (id INT PRIMARY KEY, label TEXT)",
            "INSERT INTO delete_old_new_target VALUES (1, 'old')",
        ],
    );

    // Act
    let deleted = fixture.execute(
        "DELETE FROM delete_old_new_target WHERE id = 1 RETURNING WITH (OLD AS old_row, NEW AS new_row) old_row.id, old_row.label, new_row.id, new_row.label",
    );

    // Assert
    assert_eq!(
        deleted.expect("DELETE RETURNING OLD and NEW rows").rows(),
        &[vec![
            Value::Int64(1),
            Value::String("old".to_owned()),
            Value::Null,
            Value::Null,
        ]]
    );
}

#[test]
fn should_evaluate_a_case_guarded_returning_expression_per_updated_row() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-update-returning-case-arithmetic",
        &[
            "CREATE TABLE issue_796_returning_case (id BIGINT)",
            "INSERT INTO issue_796_returning_case VALUES (1), (2)",
        ],
    );

    // Act
    let updated = fixture.execute(
        "UPDATE issue_796_returning_case SET id = id RETURNING id, CASE WHEN id < 0 THEN id / (id - id) ELSE id END AS selected",
    );

    // Assert
    assert_eq!(
        updated
            .expect("guarded division stays unevaluated for positive rows")
            .rows(),
        &[
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(2)],
        ]
    );
}

#[test]
fn should_describe_an_empty_bigint_update_returning_expression() {
    // Arrange
    let fixture = sql_fixture(
        "issue-796-empty-update-returning-descriptor",
        &[
            "CREATE TABLE issue_796_empty_returning (id BIGINT)",
            "INSERT INTO issue_796_empty_returning VALUES (1), (2)",
        ],
    );

    // Act
    let updated = fixture.execute(
        "UPDATE issue_796_empty_returning SET id = id + 10 WHERE id < 0 RETURNING id / (id - id) AS selected",
    );

    // Assert
    let updated = updated.expect("empty UPDATE still describes its RETURNING projection");
    assert!(updated.rows().is_empty());
    assert_eq!(updated.columns()[0].type_oid, 20);
}

#[test]
fn should_assign_conflicting_columns_from_a_row_constructor() {
    // Arrange
    let fixture = sql_fixture(
        "issue-825-conflict-row-assignment",
        &[
            "CREATE TABLE conflict_row_target (id INT PRIMARY KEY, label TEXT, version INT)",
            "INSERT INTO conflict_row_target VALUES (1, 'old', 1)",
        ],
    );

    // Act
    let changed = fixture.execute(
        "INSERT INTO conflict_row_target VALUES (1, 'new', 9) ON CONFLICT (id) DO UPDATE SET (label, version) = ROW(EXCLUDED.label, EXCLUDED.version) RETURNING id, label, version",
    );

    // Assert
    assert_eq!(
        changed
            .expect("ON CONFLICT row-constructor assignment")
            .rows(),
        &[vec![
            Value::Int64(1),
            Value::String("new".to_owned()),
            Value::Int64(9),
        ]]
    );
}

#[test]
fn should_match_an_explicit_operator_class_in_a_conflict_target() {
    // Arrange
    let fixture = sql_fixture(
        "issue-825-conflict-target-opclass",
        &[
            "CREATE TABLE conflict_opclass_target (email TEXT UNIQUE, note TEXT)",
            "INSERT INTO conflict_opclass_target VALUES ('a', 'old')",
        ],
    );

    // Act
    let changed = fixture.execute(
        "INSERT INTO conflict_opclass_target VALUES ('a', 'new') ON CONFLICT (email pg_catalog.text_ops) DO UPDATE SET note = EXCLUDED.note RETURNING email, note",
    );

    // Assert
    assert_eq!(
        changed
            .expect("explicit operator-class conflict target")
            .rows(),
        &[vec![
            Value::String("a".to_owned()),
            Value::String("new".to_owned()),
        ]]
    );
}
