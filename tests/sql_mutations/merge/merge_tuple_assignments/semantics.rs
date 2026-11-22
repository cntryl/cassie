//! PostgreSQL 18 MERGE tuple assignments tracked by #823.

use cassie::types::Value;

use super::support_sql_fixture::sql_fixture;

fn merge_tuple_fixture(label: &str) -> crate::support_sql_fixture::SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE issue_823_merge_tuple_target (id INT PRIMARY KEY, label TEXT, version INT)",
            "CREATE TABLE issue_823_merge_tuple_source (id INT, label TEXT, version INT)",
            "INSERT INTO issue_823_merge_tuple_target VALUES (1, 'old', 1)",
            "INSERT INTO issue_823_merge_tuple_source VALUES (1, 'source', 9)",
        ],
    )
}

fn assert_merge_tuple_result(result: &cassie::executor::QueryResult) {
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Int64(1),
            Value::String("source".to_owned()),
            Value::Int64(9),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![23, 25, 23]
    );
}

fn correlated_merge_fixture(label: &str) -> crate::support_sql_fixture::SqlFixture {
    sql_fixture(
        label,
        &[
            "CREATE TABLE mg (id INT, w INT)",
            "CREATE TABLE mgs (id INT, v INT)",
            "INSERT INTO mg VALUES (1, 0)",
            "INSERT INTO mgs VALUES (1, 42)",
        ],
    )
}

#[test]
fn should_assign_merge_target_columns_from_a_row_constructor() {
    // Arrange
    let fixture = merge_tuple_fixture("issue-823-merge-row-assignment");

    // Act
    let merged = fixture.execute(
        "MERGE INTO issue_823_merge_tuple_target AS target \
         USING issue_823_merge_tuple_source AS source ON target.id = source.id \
         WHEN MATCHED THEN UPDATE SET (label, version) = ROW(source.label, source.version) \
         RETURNING target.id, target.label, target.version",
    );

    // Assert
    assert_merge_tuple_result(&merged.expect("assign MERGE target tuple from ROW"));
    assert_eq!(
        fixture.rows("SELECT id, label, version FROM issue_823_merge_tuple_target ORDER BY id"),
        vec![vec![
            Value::Int64(1),
            Value::String("source".to_owned()),
            Value::Int64(9),
        ]]
    );
}

#[test]
fn should_assign_merge_target_columns_from_a_scalar_subquery() {
    // Arrange
    let fixture = merge_tuple_fixture("issue-823-merge-subquery-assignment");

    // Act
    let merged = fixture.execute(
        "MERGE INTO issue_823_merge_tuple_target AS target \
         USING issue_823_merge_tuple_source AS source ON target.id = source.id \
         WHEN MATCHED THEN UPDATE SET (label, version) = ( \
           SELECT source.label, source.version \
         ) RETURNING target.id, target.label, target.version",
    );

    // Assert
    assert_merge_tuple_result(&merged.expect("assign MERGE target tuple from subquery"));
    assert_eq!(
        fixture.rows("SELECT id, label, version FROM issue_823_merge_tuple_target ORDER BY id"),
        vec![vec![
            Value::Int64(1),
            Value::String("source".to_owned()),
            Value::Int64(9),
        ]]
    );
}

#[test]
fn should_evaluate_a_correlated_merge_set_subquery_for_each_source_row() {
    // Arrange
    let fixture = correlated_merge_fixture("issue-1458-correlated-merge-set");

    // Act
    fixture
        .execute(
            "MERGE INTO mg t USING mgs s ON t.id = s.id \
             WHEN MATCHED THEN UPDATE SET w = (SELECT x.v FROM mgs x WHERE x.id = s.id)",
        )
        .expect("evaluate correlated SET subquery per source row");

    // Assert
    assert_eq!(
        fixture.rows("SELECT * FROM mg"),
        vec![vec![Value::Int64(1), Value::Int64(42)]]
    );
}

#[test]
fn should_evaluate_a_correlated_merge_set_subquery_for_each_matching_source_row() {
    // Arrange
    let fixture = correlated_merge_fixture("issue-1458-correlated-merge-set-per-row");
    fixture
        .execute("INSERT INTO mg VALUES (2, 0)")
        .expect("add second target row");
    fixture
        .execute("INSERT INTO mgs VALUES (2, 84)")
        .expect("add second source row");

    // Act
    fixture
        .execute(
            "MERGE INTO mg t USING mgs s ON t.id = s.id \
             WHEN MATCHED THEN UPDATE SET w = (SELECT x.v FROM mgs x WHERE x.id = s.id)",
        )
        .expect("evaluate correlated SET subquery for each matching source row");

    // Assert
    assert_eq!(
        fixture.rows("SELECT * FROM mg ORDER BY id"),
        vec![
            vec![Value::Int64(1), Value::Int64(42)],
            vec![Value::Int64(2), Value::Int64(84)],
        ]
    );
}

#[test]
fn should_insert_values_from_a_correlated_merge_subquery_per_source_row() {
    // Arrange
    let fixture = sql_fixture(
        "issue-1458-correlated-merge-insert-values",
        &[
            "CREATE TABLE mg (id INT, w INT)",
            "CREATE TABLE mgs (id INT, v INT)",
            "INSERT INTO mgs VALUES (1, 42), (2, 7)",
        ],
    );

    // Act
    fixture
        .execute(
            "MERGE INTO mg t USING mgs s ON t.id = s.id \
             WHEN NOT MATCHED THEN INSERT (id, w) VALUES \
             (s.id, (SELECT x.v FROM mgs x WHERE x.id = s.id))",
        )
        .expect("insert each correlated source value");

    // Assert
    assert_eq!(
        fixture.rows("SELECT id, w FROM mg ORDER BY id"),
        vec![
            vec![Value::Int64(1), Value::Int64(42)],
            vec![Value::Int64(2), Value::Int64(7)],
        ]
    );
}

#[test]
fn should_keep_uncorrelated_merge_set_subqueries_working() {
    // Arrange
    let fixture = correlated_merge_fixture("issue-1458-uncorrelated-merge-set");

    // Act
    fixture
        .execute(
            "MERGE INTO mg t USING mgs s ON t.id = s.id \
             WHEN MATCHED THEN UPDATE SET w = (SELECT 7)",
        )
        .expect("evaluate uncorrelated SET subquery");

    // Assert
    assert_eq!(
        fixture.rows("SELECT * FROM mg"),
        vec![vec![Value::Int64(1), Value::Int64(7)]]
    );
}

#[test]
fn should_reject_a_correlated_merge_set_subquery_that_returns_multiple_rows() {
    // Arrange
    let fixture = correlated_merge_fixture("issue-1458-correlated-merge-set-cardinality");
    fixture
        .execute("INSERT INTO mgs VALUES (1, 43)")
        .expect("add second matching subquery row");

    // Act
    let error = fixture.error(
        "MERGE INTO mg t USING (SELECT 1 AS id) s ON t.id = s.id \
         WHEN MATCHED THEN UPDATE SET w = (SELECT x.v FROM mgs x WHERE x.id = s.id)",
    );

    // Assert
    assert_eq!(error.sql_state(), "21000");
}
