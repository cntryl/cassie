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
