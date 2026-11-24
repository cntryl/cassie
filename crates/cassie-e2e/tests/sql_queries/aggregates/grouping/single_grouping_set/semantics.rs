use cassie::types::Value;

#[test]
fn should_execute_a_single_expression_grouping_set() {
    // Arrange
    let fixture = super::super::super::support_sql_fixture::sql_fixture(
        "issue-804-single-expression-grouping-set",
        &[
            "CREATE TABLE r (n BIGINT)",
            "INSERT INTO r VALUES (5), (5), (NULL), (-2), (0), (7)",
        ],
    );

    // Act
    let result =
        fixture.execute("SELECT n FROM r GROUP BY GROUPING SETS (n) ORDER BY n NULLS LAST");

    // Assert
    assert_eq!(
        result.expect("single expression grouping set").rows(),
        &[
            vec![Value::Int64(-2)],
            vec![Value::Int64(0)],
            vec![Value::Int64(5)],
            vec![Value::Int64(7)],
            vec![Value::Null],
        ]
    );
}
