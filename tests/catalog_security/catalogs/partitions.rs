//! Partition definition helpers over live partitioned relations.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_report_partition_definitions_given_partitioned_tables_when_definition_helpers_are_called()
{
    // Arrange
    let fixture = sql_fixture("issue-817-partition-definition-helpers", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute(
            "CREATE TABLE issue_817_partition_parent (id INT, occurred_on DATE) \
             PARTITION BY RANGE (occurred_on)",
        )
        .expect("create range-partitioned definition fixture");
    fixture
        .execute(
            "CREATE TABLE issue_817_partition_2024 \
             PARTITION OF issue_817_partition_parent \
             FOR VALUES FROM ('2024-01-01') TO ('2025-01-01')",
        )
        .expect("create bounded date partition");

    // Act
    let result = fixture
        .execute(
            "SELECT pg_get_partkeydef('issue_817_partition_parent'::regclass), \
                    pg_get_partition_constraintdef('issue_817_partition_2024'::regclass)",
        )
        .expect("describe the live partition key and child constraint");

    // Assert
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        [25, 25]
    );
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("RANGE (occurred_on)".to_owned()),
            Value::String(
                "((occurred_on IS NOT NULL) AND (occurred_on >= '2024-01-01'::date) AND (occurred_on < '2025-01-01'::date))".to_owned(),
            ),
        ]]
    );
}
