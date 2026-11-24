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
fn should_allow_qualified_unique_outer_columns_inside_subqueries() {
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
             AND EXISTS (SELECT 1 FROM ms2 WHERE ms2.z = w + 1)",
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

#[test]
fn should_reject_an_update_scalar_subquery_with_multiple_rows_without_mutation() {
    // Arrange
    let fixture = sql_fixture(
        "dml-update-scalar-multirow",
        &[
            "CREATE TABLE scalar_update_target (id INT, v INT)",
            "CREATE TABLE scalar_update_values (v INT)",
            "INSERT INTO scalar_update_target VALUES (1, 10), (2, 20)",
            "INSERT INTO scalar_update_values VALUES (7), (8)",
        ],
    );

    // Act
    let error =
        fixture.error("UPDATE scalar_update_target SET v = (SELECT v FROM scalar_update_values)");

    // Assert
    assert_eq!(error.sql_state(), "21000");
    assert_eq!(
        fixture.rows("SELECT id, v FROM scalar_update_target ORDER BY id"),
        vec![
            vec![Value::Int64(1), Value::Int64(10)],
            vec![Value::Int64(2), Value::Int64(20)],
        ]
    );
}

#[test]
fn should_assign_null_from_an_update_scalar_subquery() {
    // Arrange
    let fixture = sql_fixture(
        "dml-update-scalar-null",
        &[
            "CREATE TABLE scalar_update_null_target (id INT, v INT)",
            "INSERT INTO scalar_update_null_target VALUES (1, 10), (2, 20)",
        ],
    );

    // Act
    fixture
        .execute(
            "UPDATE scalar_update_null_target SET v = (SELECT v FROM scalar_update_null_target WHERE id = 99)",
        )
        .expect("assign NULL from a zero-row scalar subquery");

    // Assert
    assert_eq!(
        fixture.rows("SELECT id, v FROM scalar_update_null_target ORDER BY id"),
        vec![
            vec![Value::Int64(1), Value::Null],
            vec![Value::Int64(2), Value::Null],
        ]
    );
}

#[test]
fn should_evaluate_a_correlated_update_scalar_subquery_per_target_row() {
    // Arrange
    let fixture = sql_fixture(
        "dml-update-scalar-correlated",
        &[
            "CREATE TABLE scalar_update_correlated_target (id INT, v INT)",
            "CREATE TABLE scalar_update_correlated_source (id INT, v INT)",
            "INSERT INTO scalar_update_correlated_target VALUES (1, 10), (2, 20)",
            "INSERT INTO scalar_update_correlated_source VALUES (1, 100), (2, 200)",
        ],
    );

    // Act
    fixture
        .execute(
            "UPDATE scalar_update_correlated_target t SET v = \
             (SELECT s.v FROM scalar_update_correlated_source s WHERE s.id = t.id)",
        )
        .expect("evaluate correlated UPDATE scalar subqueries per row");

    // Assert
    assert_eq!(
        fixture.rows("SELECT id, v FROM scalar_update_correlated_target ORDER BY id"),
        vec![
            vec![Value::Int64(1), Value::Int64(100)],
            vec![Value::Int64(2), Value::Int64(200)],
        ]
    );
}

#[test]
fn should_evaluate_a_correlated_delete_scalar_subquery_per_target_row() {
    // Arrange
    let fixture = sql_fixture(
        "dml-delete-scalar-correlated",
        &[
            "CREATE TABLE scalar_delete_correlated_target (id INT, keep INT)",
            "CREATE TABLE scalar_delete_correlated_source (id INT, keep INT)",
            "INSERT INTO scalar_delete_correlated_target VALUES (1, 1), (2, 0)",
            "INSERT INTO scalar_delete_correlated_source VALUES (1, 1), (2, 0)",
        ],
    );

    // Act
    fixture
        .execute(
            "DELETE FROM scalar_delete_correlated_target t WHERE \
             (SELECT s.keep FROM scalar_delete_correlated_source s WHERE s.id = t.id) = 1",
        )
        .expect("evaluate correlated DELETE scalar subqueries per row");

    // Assert
    assert_eq!(
        fixture.rows("SELECT id, keep FROM scalar_delete_correlated_target ORDER BY id"),
        vec![vec![Value::Int64(2), Value::Int64(0)]]
    );
}

#[test]
fn should_reject_a_delete_scalar_subquery_with_multiple_rows_without_mutation() {
    // Arrange
    let fixture = sql_fixture(
        "dml-delete-scalar-multirow",
        &[
            "CREATE TABLE scalar_delete_target (id INT, keep INT)",
            "CREATE TABLE scalar_delete_values (keep INT)",
            "INSERT INTO scalar_delete_target VALUES (1, 1), (2, 0)",
            "INSERT INTO scalar_delete_values VALUES (1), (1)",
        ],
    );

    // Act
    let error = fixture.error(
        "DELETE FROM scalar_delete_target WHERE (SELECT keep FROM scalar_delete_values) = 1",
    );

    // Assert
    assert_eq!(error.sql_state(), "21000");
    assert_eq!(
        fixture.rows("SELECT id, keep FROM scalar_delete_target ORDER BY id"),
        vec![
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(0)],
        ]
    );
}

#[test]
fn should_delete_nothing_when_a_delete_scalar_subquery_returns_no_rows() {
    // Arrange
    let fixture = sql_fixture(
        "dml-delete-scalar-null",
        &[
            "CREATE TABLE scalar_delete_null_target (id INT, keep INT)",
            "INSERT INTO scalar_delete_null_target VALUES (1, 1), (2, 0)",
        ],
    );

    // Act
    fixture
        .execute(
            "DELETE FROM scalar_delete_null_target WHERE \
             (SELECT keep FROM scalar_delete_null_target WHERE id = 99) = 1",
        )
        .expect("a zero-row scalar subquery yields SQL NULL and deletes nothing");

    // Assert
    assert_eq!(
        fixture.rows("SELECT id, keep FROM scalar_delete_null_target ORDER BY id"),
        vec![
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(0)]
        ]
    );
}

#[test]
fn should_reject_a_joined_update_scalar_subquery_with_multiple_rows_without_mutation() {
    // Arrange
    let fixture = sql_fixture(
        "dml-joined-update-scalar-multirow",
        &[
            "CREATE TABLE joined_scalar_update_target (id INT, v INT)",
            "CREATE TABLE joined_scalar_update_source (id INT, v INT)",
            "CREATE TABLE joined_scalar_update_values (v INT)",
            "INSERT INTO joined_scalar_update_target VALUES (1, 10), (2, 20)",
            "INSERT INTO joined_scalar_update_source VALUES (1, 100)",
            "INSERT INTO joined_scalar_update_values VALUES (7), (8)",
        ],
    );

    // Act
    let error = fixture.error(
        "UPDATE joined_scalar_update_target t SET v = \
         (SELECT v FROM joined_scalar_update_values) \
         FROM joined_scalar_update_source s WHERE t.id = s.id",
    );

    // Assert
    assert_eq!(error.sql_state(), "21000");
    assert_eq!(
        fixture.rows("SELECT id, v FROM joined_scalar_update_target ORDER BY id"),
        vec![
            vec![Value::Int64(1), Value::Int64(10)],
            vec![Value::Int64(2), Value::Int64(20)]
        ]
    );
}

#[test]
fn should_delete_nothing_for_a_joined_zero_row_scalar_subquery() {
    // Arrange
    let fixture = sql_fixture(
        "dml-joined-delete-scalar-null",
        &[
            "CREATE TABLE joined_scalar_delete_target (id INT, keep INT)",
            "CREATE TABLE joined_scalar_delete_source (id INT)",
            "INSERT INTO joined_scalar_delete_target VALUES (1, 1), (2, 0)",
            "INSERT INTO joined_scalar_delete_source VALUES (1)",
        ],
    );

    // Act
    fixture
        .execute(
            "DELETE FROM joined_scalar_delete_target t USING joined_scalar_delete_source s \
             WHERE t.id = s.id AND \
             (SELECT keep FROM joined_scalar_delete_target WHERE id = 99) = 1",
        )
        .expect("a zero-row joined scalar subquery yields SQL NULL and deletes nothing");

    // Assert
    assert_eq!(
        fixture.rows("SELECT id, keep FROM joined_scalar_delete_target ORDER BY id"),
        vec![
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(0)]
        ]
    );
}

#[test]
fn should_evaluate_a_joined_correlated_update_scalar_subquery_per_target_row() {
    // Arrange
    let fixture = sql_fixture(
        "dml-joined-update-scalar-correlated",
        &[
            "CREATE TABLE joined_scalar_update_target (id INT, v INT)",
            "CREATE TABLE joined_scalar_update_source (id INT, v INT)",
            "INSERT INTO joined_scalar_update_target VALUES (1, 10), (2, 20)",
            "INSERT INTO joined_scalar_update_source VALUES (1, 100), (2, 200)",
        ],
    );

    // Act
    fixture
        .execute(
            "UPDATE joined_scalar_update_target t SET v = \
             (SELECT s.v FROM joined_scalar_update_source s WHERE s.id = t.id) \
             FROM joined_scalar_update_source s WHERE t.id = s.id",
        )
        .expect("evaluate joined correlated UPDATE scalar subqueries per row");

    // Assert
    assert_eq!(
        fixture.rows("SELECT id, v FROM joined_scalar_update_target ORDER BY id"),
        vec![
            vec![Value::Int64(1), Value::Int64(100)],
            vec![Value::Int64(2), Value::Int64(200)]
        ]
    );
}

#[test]
fn should_reject_a_joined_delete_scalar_subquery_with_multiple_rows_without_mutation() {
    // Arrange
    let fixture = sql_fixture(
        "dml-joined-delete-scalar-multirow",
        &[
            "CREATE TABLE joined_scalar_delete_target (id INT, keep INT)",
            "CREATE TABLE joined_scalar_delete_source (id INT)",
            "CREATE TABLE joined_scalar_delete_values (keep INT)",
            "INSERT INTO joined_scalar_delete_target VALUES (1, 1), (2, 0)",
            "INSERT INTO joined_scalar_delete_source VALUES (1)",
            "INSERT INTO joined_scalar_delete_values VALUES (1), (1)",
        ],
    );

    // Act
    let error = fixture.error(
        "DELETE FROM joined_scalar_delete_target t USING joined_scalar_delete_source s \
         WHERE t.id = s.id AND (SELECT keep FROM joined_scalar_delete_values) = 1",
    );

    // Assert
    assert_eq!(error.sql_state(), "21000");
    assert_eq!(
        fixture.rows("SELECT id, keep FROM joined_scalar_delete_target ORDER BY id"),
        vec![
            vec![Value::Int64(1), Value::Int64(1)],
            vec![Value::Int64(2), Value::Int64(0)]
        ]
    );
}

#[test]
fn should_reject_a_plain_update_scalar_type_error_without_mutation() {
    // Arrange
    let fixture = sql_fixture(
        "dml-update-scalar-type-error",
        &[
            "CREATE TABLE scalar_update_type_target (id INT, v INT)",
            "INSERT INTO scalar_update_type_target VALUES (1, 10), (2, 20)",
        ],
    );

    // Act
    let error = fixture.error("UPDATE scalar_update_type_target SET v = (SELECT 'not-an-int')");

    // Assert
    assert_eq!(error.sql_state(), "22P02");
    assert_eq!(
        fixture.rows("SELECT id, v FROM scalar_update_type_target ORDER BY id"),
        vec![
            vec![Value::Int64(1), Value::Int64(10)],
            vec![Value::Int64(2), Value::Int64(20)]
        ]
    );
}

#[test]
fn should_reject_a_joined_delete_binder_error_without_mutation() {
    // Arrange
    let fixture = sql_fixture(
        "dml-joined-delete-binder-error",
        &[
            "CREATE TABLE joined_delete_binder_target (id INT, keep INT)",
            "CREATE TABLE joined_delete_binder_source (id INT)",
            "INSERT INTO joined_delete_binder_target VALUES (1, 1)",
            "INSERT INTO joined_delete_binder_source VALUES (1)",
        ],
    );

    // Act
    let error = fixture.error(
        "DELETE FROM joined_delete_binder_target t \
         USING joined_delete_binder_source s \
         WHERE t.id = s.id AND missing_alias.keep = 1",
    );

    // Assert
    assert_eq!(error.sql_state(), "42703");
    assert_eq!(
        fixture.rows("SELECT id, keep FROM joined_delete_binder_target"),
        vec![vec![Value::Int64(1), Value::Int64(1)]]
    );
}

#[test]
fn should_assign_null_from_a_joined_update_scalar_subquery() {
    // Arrange
    let fixture = sql_fixture(
        "dml-joined-update-scalar-null",
        &[
            "CREATE TABLE joined_scalar_update_null_target (id INT, v INT)",
            "CREATE TABLE joined_scalar_update_null_source (id INT)",
            "INSERT INTO joined_scalar_update_null_target VALUES (1, 10), (2, 20)",
            "INSERT INTO joined_scalar_update_null_source VALUES (1), (2)",
        ],
    );

    // Act
    fixture
        .execute(
            "UPDATE joined_scalar_update_null_target t SET v = \
             (SELECT v FROM joined_scalar_update_null_target WHERE id = 99) \
             FROM joined_scalar_update_null_source s WHERE t.id = s.id",
        )
        .expect("assign NULL from a joined zero-row scalar subquery");

    // Assert
    assert_eq!(
        fixture.rows("SELECT id, v FROM joined_scalar_update_null_target ORDER BY id"),
        vec![
            vec![Value::Int64(1), Value::Null],
            vec![Value::Int64(2), Value::Null]
        ]
    );
}

#[test]
fn should_evaluate_a_joined_correlated_delete_scalar_subquery_per_target_row() {
    // Arrange
    let fixture = sql_fixture(
        "dml-joined-delete-scalar-correlated",
        &[
            "CREATE TABLE joined_scalar_delete_correlated_target (id INT, keep INT)",
            "CREATE TABLE joined_scalar_delete_correlated_source (id INT, keep INT)",
            "INSERT INTO joined_scalar_delete_correlated_target VALUES (1, 1), (2, 0)",
            "INSERT INTO joined_scalar_delete_correlated_source VALUES (1, 1), (2, 0)",
        ],
    );

    // Act
    fixture
        .execute(
            "DELETE FROM joined_scalar_delete_correlated_target t \
             USING joined_scalar_delete_correlated_source s \
             WHERE t.id = s.id AND \
             (SELECT i.keep FROM joined_scalar_delete_correlated_source i WHERE i.id = t.id) = 1",
        )
        .expect("evaluate joined correlated DELETE scalar subqueries per row");

    // Assert
    assert_eq!(
        fixture.rows("SELECT id, keep FROM joined_scalar_delete_correlated_target ORDER BY id"),
        vec![vec![Value::Int64(2), Value::Int64(0)]]
    );
}
