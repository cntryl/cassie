#[test]
fn should_report_created_relation_catalog_identities() {
    // Arrange
    let fixture = created_object_catalog_fixture("issue-817-created-relation-identities");

    // Act
    let relations = fixture
        .execute(
            "SELECT c.relname, c.relkind::text, c.relpersistence::text \
             FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.nspname = c.relnamespace \
             WHERE n.nspname = 'issue_817_catalog_probe' \
               AND c.relname IN ('items', 'issue_817_amount_idx', 'items_id_seq', 'items_pkey') \
             ORDER BY c.relname",
        )
        .expect("read created relation identities");

    // Assert
    assert_eq!(
        relations.rows(),
        vec![
            vec![
                Value::String("issue_817_amount_idx".to_owned()),
                Value::String("i".to_owned()),
                Value::String("p".to_owned()),
            ],
            vec![
                Value::String("items".to_owned()),
                Value::String("r".to_owned()),
                Value::String("p".to_owned()),
            ],
            vec![
                Value::String("items_id_seq".to_owned()),
                Value::String("S".to_owned()),
                Value::String("p".to_owned()),
            ],
            vec![
                Value::String("items_pkey".to_owned()),
                Value::String("i".to_owned()),
                Value::String("p".to_owned()),
            ],
        ]
    );
}
