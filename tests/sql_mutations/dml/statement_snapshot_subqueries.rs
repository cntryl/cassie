//! Subqueries in a mutation read the data as it was when the statement began.

use cassie::types::Value;

use super::support_sql_fixture::sql_fixture;

fn ints(values: &[i64]) -> Vec<Vec<Value>> {
    values
        .iter()
        .map(|value| vec![Value::Int64(*value)])
        .collect()
}

#[test]
fn should_evaluate_a_merge_when_condition_subquery_against_the_statement_start() {
    // Arrange
    let fixture = sql_fixture(
        "merge-when-snapshot",
        &[
            "CREATE TABLE tt (id INT, v INT)",
            "CREATE TABLE src3 (id INT, v INT)",
            "INSERT INTO tt VALUES (1, 1), (2, 2)",
            "INSERT INTO src3 VALUES (10, 0), (11, 0), (12, 0)",
        ],
    );

    // Act
    fixture
        .execute(
            "MERGE INTO tt USING src3 ON tt.id = src3.id \
             WHEN NOT MATCHED AND (SELECT count(*) FROM tt) < 3 \
             THEN INSERT (id, v) VALUES (src3.id, src3.v)",
        )
        .expect("merge");

    // Assert
    assert_eq!(fixture.rows("SELECT count(*) FROM tt"), ints(&[5]));
}

#[test]
fn should_evaluate_a_merge_set_subquery_against_the_statement_start() {
    // Arrange
    let fixture = sql_fixture(
        "merge-set-snapshot",
        &[
            "CREATE TABLE tt (id INT, v INT)",
            "CREATE TABLE src3 (id INT, v INT)",
            "INSERT INTO tt VALUES (1, 1), (2, 2)",
            "INSERT INTO src3 VALUES (1, 0), (2, 0)",
        ],
    );

    // Act
    fixture
        .execute(
            "MERGE INTO tt USING src3 ON tt.id = src3.id \
             WHEN MATCHED THEN UPDATE SET v = (SELECT sum(v) FROM tt)",
        )
        .expect("merge");

    // Assert
    assert_eq!(fixture.rows("SELECT v FROM tt ORDER BY id"), ints(&[3, 3]));
}

#[test]
fn should_evaluate_a_merge_insert_values_subquery_against_the_statement_start() {
    // Arrange
    let fixture = sql_fixture(
        "merge-values-snapshot",
        &[
            "CREATE TABLE tt (id INT, v INT)",
            "CREATE TABLE src3 (id INT, v INT)",
            "INSERT INTO tt VALUES (1, 1), (2, 2)",
            "INSERT INTO src3 VALUES (10, 0), (11, 0), (12, 0)",
        ],
    );

    // Act
    fixture
        .execute(
            "MERGE INTO tt USING src3 ON tt.id = src3.id \
             WHEN NOT MATCHED THEN INSERT (id, v) VALUES (src3.id, (SELECT count(*) FROM tt))",
        )
        .expect("merge");

    // Assert
    assert_eq!(
        fixture.rows("SELECT v FROM tt WHERE id >= 10 ORDER BY id"),
        ints(&[2, 2, 2])
    );
}

#[test]
fn should_evaluate_an_update_set_subquery_against_the_statement_start() {
    // Arrange
    let fixture = sql_fixture(
        "update-set-snapshot",
        &[
            "CREATE TABLE tt (id INT, v INT)",
            "INSERT INTO tt VALUES (1, 1), (2, 2)",
        ],
    );

    // Act
    fixture
        .execute("UPDATE tt SET v = (SELECT sum(v) FROM tt)")
        .expect("update");

    // Assert
    assert_eq!(fixture.rows("SELECT v FROM tt ORDER BY id"), ints(&[3, 3]));
}

#[test]
fn should_evaluate_a_delete_where_subquery_against_the_statement_start() {
    // Arrange
    let fixture = sql_fixture(
        "delete-where-snapshot",
        &[
            "CREATE TABLE tt (id INT, v INT)",
            "INSERT INTO tt VALUES (1, 1), (2, 2), (3, 3), (4, 4)",
        ],
    );

    // Act
    fixture
        .execute("DELETE FROM tt WHERE id > (SELECT avg(id) FROM tt)")
        .expect("delete");

    // Assert
    assert_eq!(fixture.rows("SELECT id FROM tt ORDER BY id"), ints(&[1, 2]));
}

#[test]
fn should_insert_from_a_select_of_the_same_table_without_reading_new_rows() {
    // Arrange
    let fixture = sql_fixture(
        "insert-select-snapshot",
        &[
            "CREATE TABLE tt (id INT, v INT)",
            "INSERT INTO tt VALUES (1, 1), (2, 2)",
        ],
    );

    // Act
    fixture
        .execute("INSERT INTO tt SELECT id + 100, v FROM tt")
        .expect("insert");

    // Assert
    assert_eq!(fixture.rows("SELECT count(*) FROM tt"), ints(&[4]));
}

#[test]
fn should_evaluate_an_on_conflict_set_subquery_against_the_statement_start() {
    // Arrange
    let fixture = sql_fixture(
        "on-conflict-snapshot",
        &[
            "CREATE TABLE u (id INT PRIMARY KEY, x INT)",
            "INSERT INTO u VALUES (1, 1), (2, 1)",
        ],
    );

    // Act
    fixture
        .execute(
            "INSERT INTO u VALUES (1, 0), (2, 0) \
             ON CONFLICT (id) DO UPDATE SET x = (SELECT sum(x) FROM u)",
        )
        .expect("upsert");

    // Assert
    assert_eq!(fixture.rows("SELECT x FROM u ORDER BY id"), ints(&[2, 2]));
}
