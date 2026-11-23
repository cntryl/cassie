#[test]
fn should_report_created_table_constraint_identities() {
    // Arrange
    let fixture = created_object_catalog_fixture("issue-817-created-table-constraints");

    // Act
    let constraints = fixture
        .execute(
            "SELECT con.conname, con.contype::text, con.convalidated \
             FROM pg_catalog.pg_constraint con \
             JOIN pg_catalog.pg_class c ON con.conrelid = c.oid \
             JOIN pg_catalog.pg_namespace n ON c.relnamespace = n.oid \
             WHERE c.relname = 'items' AND n.nspname = 'issue_817_catalog_probe' \
             ORDER BY con.conname",
        )
        .expect("read table constraints");

    // Assert
    assert_eq!(
        constraints.rows(),
        vec![
            vec![
                Value::String("issue_817_amount_positive".to_owned()),
                Value::String("c".to_owned()),
                Value::Bool(true),
            ],
            vec![
                Value::String("items_amount_not_null".to_owned()),
                Value::String("n".to_owned()),
                Value::Bool(true),
            ],
            vec![
                Value::String("items_id_not_null".to_owned()),
                Value::String("n".to_owned()),
                Value::Bool(true),
            ],
            vec![
                Value::String("items_pkey".to_owned()),
                Value::String("p".to_owned()),
                Value::Bool(true),
            ],
        ]
    );
}
