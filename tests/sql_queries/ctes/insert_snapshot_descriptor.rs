//! INSERT RETURNING ownership and statement-snapshot visibility for #834.

use cassie::types::Value;

use crate::support_sql_fixture::sql_fixture;

#[test]
fn should_keep_insert_returning_rows_out_of_a_sibling_cte_snapshot() {
    // Arrange
    let fixture = sql_fixture(
        "issue-834-insert-cte-snapshot-descriptor",
        &[
            "CREATE TABLE issue_834_insert_snapshot (id INT PRIMARY KEY, payload TEXT)",
            "INSERT INTO issue_834_insert_snapshot VALUES (1, 'kept'), (2, 'two')",
        ],
    );

    // Act
    let observed = fixture.execute(
        "WITH inserted AS ( \
             INSERT INTO issue_834_insert_snapshot VALUES (3, NULL) \
             RETURNING id, payload \
         ), snapshot AS ( \
             SELECT id, payload FROM issue_834_insert_snapshot \
         ) \
         SELECT 'returning' AS view, id, payload FROM inserted \
         UNION ALL \
         SELECT 'snapshot', id, payload FROM snapshot \
         ORDER BY view, id",
    );
    let stored = fixture.execute("SELECT id, payload FROM issue_834_insert_snapshot ORDER BY id");

    // Assert
    let observed = observed.expect("read INSERT RETURNING and sibling statement snapshot");
    assert_eq!(
        observed
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 23, 25],
        "the CTE relation must retain text, int4, and text PostgreSQL descriptors"
    );
    assert_eq!(
        observed.rows(),
        &[
            vec![
                Value::String("returning".to_owned()),
                Value::Int64(3),
                Value::Null,
            ],
            vec![
                Value::String("snapshot".to_owned()),
                Value::Int64(1),
                Value::String("kept".to_owned()),
            ],
            vec![
                Value::String("snapshot".to_owned()),
                Value::Int64(2),
                Value::String("two".to_owned()),
            ],
        ],
        "the sibling SELECT sees the original statement snapshot while RETURNING owns the inserted NULL row"
    );
    assert_eq!(
        stored.expect("read target after INSERT CTE").rows(),
        &[
            vec![Value::Int64(1), Value::String("kept".to_owned())],
            vec![Value::Int64(2), Value::String("two".to_owned())],
            vec![Value::Int64(3), Value::Null],
        ]
    );
}
