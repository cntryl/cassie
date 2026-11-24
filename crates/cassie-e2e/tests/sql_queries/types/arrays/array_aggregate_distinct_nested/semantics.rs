//! PostgreSQL 18.6 DISTINCT and ordering semantics for array-valued aggregates.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_order_distinct_array_inputs_lexicographically() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-array-aggregate-distinct-order-nested",
        &[
            "CREATE TABLE issue_811_nested_array_aggregate (item INT4[])",
            "INSERT INTO issue_811_nested_array_aggregate VALUES ('[0:1]={2,1}'::INT4[]), ('[0:1]={1,4}'::INT4[]), ('[0:1]={2,1}'::INT4[])",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT array_agg(DISTINCT item ORDER BY item)::TEXT, \
                array_dims(array_agg(DISTINCT item ORDER BY item)), \
                pg_typeof(array_agg(DISTINCT item ORDER BY item))::OID \
         FROM issue_811_nested_array_aggregate",
    );

    // Assert
    let result = result.expect("array_agg deduplicates and sorts array-valued inputs");
    assert_eq!(
        result.rows(),
        &[vec![
            Value::String("[1:2][0:1]={{1,4},{2,1}}".to_owned()),
            Value::String("[1:2][0:1]".to_owned()),
            Value::Int64(1007),
        ]]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 25, 26]
    );
}
