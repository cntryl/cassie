//! Byte-oriented STRING_AGG ordering, NULL, and result type contract for #811.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_preserve_ordered_bytea_string_aggregate() {
    // Arrange
    let fixture = sql_fixture(
        "issue-811-bytea-string-aggregate-order-type",
        &[
            "CREATE TABLE issue_811_bytea_string_order_values (bucket TEXT, position INT, payload BYTEA)",
            "INSERT INTO issue_811_bytea_string_order_values VALUES ('mixed',3,decode('ff62','hex')),('mixed',1,decode('6100','hex')),('mixed',2,NULL),('mixed',4,decode('63','hex')),('all_null',1,NULL)",
        ],
    );

    // Act
    let grouped = fixture.execute(
        "SELECT bucket, \
                string_agg(payload, decode('ff','hex') ORDER BY position) \
         FROM issue_811_bytea_string_order_values \
         GROUP BY bucket ORDER BY bucket",
    );
    let empty = fixture.execute(
        "SELECT string_agg(payload, decode('ff','hex')) \
         FROM issue_811_bytea_string_order_values WHERE FALSE",
    );

    // Assert
    let grouped = grouped.expect("ordered bytea STRING_AGG groups");
    assert_eq!(
        grouped.rows(),
        &[
            vec![Value::String("all_null".to_owned()), Value::Null],
            vec![
                Value::String("mixed".to_owned()),
                Value::String(r"\x6100ffff62ff63".to_owned()),
            ],
        ]
    );
    assert_eq!(
        grouped
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 17]
    );

    let empty = empty.expect("bytea STRING_AGG over no input rows");
    assert_eq!(empty.rows(), &[vec![Value::Null]]);
    assert_eq!(
        empty
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![17]
    );
}
