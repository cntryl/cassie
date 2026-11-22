//! A column both the target and the source declare is ambiguous when unqualified.

use cassie::types::Value;

use super::support_sql_fixture::{sql_fixture, SqlFixture};

fn fixture(label: &str) -> SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE mt (id INT, v INT, w INT)",
            "CREATE TABLE ms (id INT, v INT)",
            "CREATE TABLE ms2 (id INT, z INT)",
            "INSERT INTO mt VALUES (1, 10, 0)",
            "INSERT INTO ms VALUES (1, 99)",
            "INSERT INTO ms2 VALUES (1, 5)",
        ],
    )
}

fn assert_ambiguous(fixture: &SqlFixture, sql: &str) {
    let error = fixture.error(sql);
    assert_eq!(error.sql_state(), "42702", "{sql}");
    assert!(error.to_string().ends_with("'v' is ambiguous"), "{sql}");
}

#[test]
fn should_reject_an_ambiguous_column_in_an_update_from_set_value() {
    // Arrange
    let fixture = fixture("ambiguity-update-set");

    // Act
    let sql = "UPDATE mt SET w = v FROM ms WHERE mt.id = ms.id";

    // Assert
    assert_ambiguous(&fixture, sql);
}

#[test]
fn should_reject_an_ambiguous_column_in_an_update_from_filter() {
    // Arrange
    let fixture = fixture("ambiguity-update-where");

    // Act
    let sql = "UPDATE mt SET w = 1 FROM ms WHERE mt.id = ms.id AND v = 10";

    // Assert
    assert_ambiguous(&fixture, sql);
}

#[test]
fn should_reject_an_ambiguous_column_in_an_update_from_returning() {
    // Arrange
    let fixture = fixture("ambiguity-update-returning");

    // Act
    let sql = "UPDATE mt SET w = 1 FROM ms WHERE mt.id = ms.id RETURNING v";

    // Assert
    assert_ambiguous(&fixture, sql);
}

#[test]
fn should_reject_an_ambiguous_column_in_a_delete_using_filter() {
    // Arrange
    let fixture = fixture("ambiguity-delete-using");

    // Act
    let sql = "DELETE FROM mt USING ms WHERE mt.id = ms.id AND v = 10";

    // Assert
    assert_ambiguous(&fixture, sql);
}

#[test]
fn should_reject_an_ambiguous_column_in_a_merge_when_condition() {
    // Arrange
    let fixture = fixture("ambiguity-merge-when");

    // Act
    let sql = "MERGE INTO mt USING ms ON mt.id = ms.id \
               WHEN MATCHED AND v = 10 THEN DELETE";

    // Assert
    assert_ambiguous(&fixture, sql);
}

#[test]
fn should_reject_an_ambiguous_column_in_a_merge_set_value() {
    // Arrange
    let fixture = fixture("ambiguity-merge-set");

    // Act
    let sql = "MERGE INTO mt USING ms ON mt.id = ms.id \
               WHEN MATCHED THEN UPDATE SET w = v";

    // Assert
    assert_ambiguous(&fixture, sql);
}

#[test]
fn should_reject_an_ambiguous_column_in_a_merge_on_condition() {
    // Arrange
    let fixture = fixture("ambiguity-merge-on");

    // Act
    let sql = "MERGE INTO mt USING ms ON mt.id = ms.id AND v = 10 \
               WHEN MATCHED THEN DELETE";

    // Assert
    assert_ambiguous(&fixture, sql);
}

#[test]
fn should_allow_the_set_target_column_to_share_a_source_column_name() {
    // Arrange
    let fixture = fixture("ambiguity-set-target");

    // Act
    fixture
        .execute("UPDATE mt SET v = ms.v FROM ms WHERE mt.id = ms.id")
        .expect("update");

    // Assert
    assert_eq!(
        fixture.rows("SELECT v FROM mt"),
        vec![vec![Value::Int64(99)]]
    );
}

#[test]
fn should_allow_qualified_references_to_shared_columns() {
    // Arrange
    let fixture = fixture("ambiguity-qualified");

    // Act
    fixture
        .execute("UPDATE mt SET w = mt.v + ms.v FROM ms WHERE mt.id = ms.id")
        .expect("update");

    // Assert
    assert_eq!(
        fixture.rows("SELECT w FROM mt"),
        vec![vec![Value::Int64(109)]]
    );
}

#[test]
fn should_allow_an_unqualified_column_only_the_source_declares() {
    // Arrange
    let fixture = fixture("ambiguity-source-only");

    // Act
    fixture
        .execute("UPDATE mt SET w = z FROM ms2 WHERE mt.id = ms2.id")
        .expect("update");

    // Assert
    assert_eq!(
        fixture.rows("SELECT w FROM mt"),
        vec![vec![Value::Int64(5)]]
    );
}

#[test]
fn should_allow_a_merge_insert_value_the_target_also_declares() {
    // Arrange
    let fixture = fixture("ambiguity-merge-insert");
    fixture.execute("DELETE FROM mt").expect("empty target");

    // Act
    fixture
        .execute(
            "MERGE INTO mt USING ms ON mt.id = ms.id \
             WHEN NOT MATCHED THEN INSERT (id, v) VALUES (ms.id, v)",
        )
        .expect("merge");

    // Assert
    assert_eq!(
        fixture.rows("SELECT v FROM mt"),
        vec![vec![Value::Int64(99)]]
    );
}
