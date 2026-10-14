//! PostgreSQL 18.6 GROUPING() behavior for expression grouping keys.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_report_grouping_bits_for_expression_keys() {
    // Arrange
    let fixture = sql_fixture(
        "issue-804-grouping-expression-keys",
        &[
            "CREATE TABLE issue_804_group_expression_values (region TEXT, product TEXT, amount INT)",
            "INSERT INTO issue_804_group_expression_values VALUES ('East','book',2), ('east','pen',3)",
        ],
    );

    // Act
    let result = fixture.execute(
        "SELECT lower(region), product, SUM(amount), GROUPING(lower(region), product) \
         FROM issue_804_group_expression_values \
         GROUP BY ROLLUP(lower(region), product) \
         ORDER BY GROUPING(lower(region), product), lower(region) NULLS FIRST, product NULLS FIRST",
    );

    // Assert
    let result = result.expect("GROUPING reports omitted expression keys at each ROLLUP level");
    assert_eq!(
        result.rows(),
        &[
            vec![
                Value::String("east".to_owned()),
                Value::String("book".to_owned()),
                Value::Int64(2),
                Value::Int64(0),
            ],
            vec![
                Value::String("east".to_owned()),
                Value::String("pen".to_owned()),
                Value::Int64(3),
                Value::Int64(0),
            ],
            vec![
                Value::String("east".to_owned()),
                Value::Null,
                Value::Int64(5),
                Value::Int64(1),
            ],
            vec![Value::Null, Value::Null, Value::Int64(5), Value::Int64(3),],
        ]
    );
    assert_eq!(
        result
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 25, 20, 23]
    );
}
