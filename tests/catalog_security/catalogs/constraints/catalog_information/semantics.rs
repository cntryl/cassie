#[test]
fn should_report_created_table_constraint_identities() {
    // Arrange
    let fixture = created_object_catalog_fixture("issue-817-created-table-constraints");

    // Act
    let constraints = fixture
        .execute(
            "SELECT conname, contype::text, convalidated \
             FROM pg_catalog.pg_constraint \
             WHERE conrelid = 'issue_817_catalog_probe.items'::regclass \
             ORDER BY conname",
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

#[test]
fn should_report_created_check_constraints_in_information_schema_extras() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-information-schema-constraint-extras",
        &["CREATE TABLE issue_817_constraint_info (\
            amount INTEGER, \
            CONSTRAINT issue_817_amount_positive CHECK (amount > 0))"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let checks = fixture.execute(
        "SELECT constraint_name, check_clause \
         FROM information_schema.check_constraints \
         WHERE constraint_schema = 'public' \
           AND constraint_name = 'issue_817_amount_positive'",
    );
    let column_usage = fixture.execute(
        "SELECT constraint_name, table_name, column_name \
         FROM information_schema.constraint_column_usage \
         WHERE constraint_schema = 'public' \
           AND constraint_name = 'issue_817_amount_positive'",
    );

    // Assert
    assert_eq!(
        checks.expect("read the created check expression").rows(),
        vec![vec![
            Value::String("issue_817_amount_positive".to_owned()),
            Value::String("(amount > 0)".to_owned()),
        ]]
    );
    assert_eq!(
        column_usage
            .expect("read the created constraint column usage")
            .rows(),
        vec![vec![
            Value::String("issue_817_amount_positive".to_owned()),
            Value::String("issue_817_constraint_info".to_owned()),
            Value::String("amount".to_owned()),
        ]]
    );
}
