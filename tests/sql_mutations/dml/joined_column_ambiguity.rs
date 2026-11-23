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
    assert_eq!(
        error.to_string(),
        "column reference \"v\" is ambiguous",
        "{sql}"
    );
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
fn should_reject_an_ambiguous_outer_column_inside_an_update_exists_subquery() {
    // Arrange
    let fixture = fixture("ambiguity-update-exists-subquery");

    // Act
    let sql = "UPDATE mt SET w = 1 FROM ms WHERE EXISTS (SELECT 1 FROM ms2 WHERE ms2.id = v)";

    // Assert
    assert_ambiguous(&fixture, sql);
}

#[test]
fn should_reject_an_ambiguous_outer_column_inside_a_delete_exists_subquery() {
    // Arrange
    let fixture = fixture("ambiguity-delete-exists-subquery");

    // Act
    let sql = "DELETE FROM mt USING ms WHERE EXISTS (SELECT 1 FROM ms2 WHERE ms2.id = v)";

    // Assert
    assert_ambiguous(&fixture, sql);
}

#[test]
fn should_reject_an_ambiguous_outer_column_inside_a_merge_exists_subquery() {
    // Arrange
    let fixture = fixture("ambiguity-merge-exists-subquery");

    // Act
    let sql = "MERGE INTO mt USING ms ON EXISTS (SELECT 1 FROM ms2 WHERE ms2.id = v) \
               WHEN MATCHED THEN UPDATE SET w = 1";

    // Assert
    assert_ambiguous(&fixture, sql);
}

#[test]
fn should_allow_qualified_and_unique_outer_columns_inside_subqueries() {
    // Arrange
    let fixture = fixture("ambiguity-qualified-subquery-references");

    // Act
    fixture
        .execute(
            "UPDATE mt SET w = 1 FROM ms WHERE mt.id = ms.id \
             AND EXISTS (SELECT 1 FROM ms2 WHERE ms2.id = mt.id)",
        )
        .expect("qualified target reference");
    fixture
        .execute(
            "UPDATE mt SET w = 3 FROM ms WHERE mt.id = ms.id \
             AND EXISTS (SELECT 1 FROM ms2 WHERE ms2.z < mt.v)",
        )
        .expect("qualified target reference");
    fixture
        .execute(
            "UPDATE mt SET w = 4 FROM ms WHERE mt.id = ms.id \
             AND EXISTS (SELECT 1 FROM ms2 WHERE ms2.z < ms.v)",
        )
        .expect("qualified source reference");
    fixture
        .execute(
            "UPDATE mt SET w = 2 FROM ms WHERE mt.id = ms.id \
             AND EXISTS (SELECT 1 FROM ms2 WHERE ms2.z = w + 5)",
        )
        .expect("unique outer reference");

    // Assert
    assert_eq!(
        fixture.rows("SELECT w FROM mt"),
        vec![vec![Value::Int64(2)]]
    );
}

#[test]
fn should_resolve_a_subquery_column_from_its_own_from_scope_first() {
    // Arrange
    let fixture = sql_fixture(
        "ambiguity-inner-subquery-scope",
        &[
            "CREATE TABLE mt (id INT, v INT, w INT)",
            "CREATE TABLE ms (id INT, v INT)",
            "CREATE TABLE ms2 (id INT, z INT, v INT)",
            "INSERT INTO mt VALUES (1, 10, 0)",
            "INSERT INTO ms VALUES (1, 99)",
            "INSERT INTO ms2 VALUES (1, 5, 5)",
        ],
    );

    // Act
    fixture
        .execute(
            "UPDATE mt SET w = 1 FROM ms WHERE mt.id = ms.id \
             AND EXISTS (SELECT 1 FROM ms2 WHERE v = 5)",
        )
        .expect("resolve the subquery's own v column");

    // Assert
    assert_eq!(
        fixture.rows("SELECT w FROM mt"),
        vec![vec![Value::Int64(1)]]
    );
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
