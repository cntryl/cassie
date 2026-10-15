#[test]
fn should_track_composite_type_catalog_lifecycle() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-composite-type-catalog-lifecycle",
        &[
            "CREATE SCHEMA issue_817_type_catalog",
            "CREATE TABLE issue_817_type_catalog.before_kind (id INTEGER)",
        ],
    );
    fixture.cassie.startup().expect("startup");
    let read_type_row = |type_name: &str| {
        let sql = format!(
            "SELECT t.oid, n.nspname, t.typname, t.typtype::text, c.relname, \
                    c.relkind::text, t.typrelid = c.oid \
             FROM pg_catalog.pg_type AS t \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = t.typnamespace \
             JOIN pg_catalog.pg_class AS c ON c.oid = t.typrelid \
             WHERE n.nspname = 'issue_817_type_catalog' AND t.typname = '{type_name}'"
        );
        fixture
            .execute(&sql)
            .expect("read the actual composite type and relation catalog rows")
            .rows()
            .to_vec()
    };
    let expected_row = |type_name: &str, oid: &Value| {
        vec![
            oid.clone(),
            Value::String("issue_817_type_catalog".to_owned()),
            Value::String(type_name.to_owned()),
            Value::String("c".to_owned()),
            Value::String(type_name.to_owned()),
            Value::String("r".to_owned()),
            Value::Bool(true),
        ]
    };
    let empty_rows = Vec::<Vec<cassie::types::Value>>::new();

    // Act
    let initial = read_type_row("before_kind");
    assert_eq!(initial.len(), 1);
    let initial_oid = initial[0][0].clone();
    assert!(matches!(&initial_oid, Value::Int64(oid) if *oid > 0));
    fixture
        .execute("BEGIN")
        .expect("begin transactional rename probe");
    fixture
        .execute(
            "ALTER TABLE issue_817_type_catalog.before_kind \
             RENAME TO rolled_back_kind",
        )
        .expect("rename the table and its composite type inside a transaction");
    let transaction_rename = read_type_row("rolled_back_kind");
    let transaction_old_name = read_type_row("before_kind");
    fixture
        .execute("ROLLBACK")
        .expect("roll back the table and composite type rename");
    let after_rollback = read_type_row("before_kind");
    let rollback_name_absent = read_type_row("rolled_back_kind");
    fixture
        .execute(
            "ALTER TABLE issue_817_type_catalog.before_kind \
             RENAME TO persisted_kind",
        )
        .expect("commit a table and composite type rename");
    let after_rename = read_type_row("persisted_kind");
    let old_name_absent = read_type_row("before_kind");
    fixture
        .execute("DROP TABLE issue_817_type_catalog.persisted_kind")
        .expect("drop the table and its automatically generated composite type");
    let after_drop = read_type_row("persisted_kind");
    let dropped_relation = fixture
        .execute(
            "SELECT count(*)::bigint \
             FROM pg_catalog.pg_class AS c \
             JOIN pg_catalog.pg_namespace AS n ON n.oid = c.relnamespace \
             WHERE n.nspname = 'issue_817_type_catalog' \
               AND c.relname = 'persisted_kind'",
        )
        .expect("confirm the dropped relation is absent from pg_class")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(initial, vec![expected_row("before_kind", &initial_oid)]);
    assert_eq!(
        transaction_rename,
        vec![expected_row("rolled_back_kind", &initial_oid)]
    );
    assert_eq!(transaction_old_name, empty_rows);
    assert_eq!(
        after_rollback,
        vec![expected_row("before_kind", &initial_oid)]
    );
    assert_eq!(rollback_name_absent, empty_rows);
    assert_eq!(
        after_rename,
        vec![expected_row("persisted_kind", &initial_oid)]
    );
    assert_eq!(old_name_absent, Vec::<Vec<cassie::types::Value>>::new());
    assert_eq!(after_drop, Vec::<Vec<cassie::types::Value>>::new());
    assert_eq!(dropped_relation, vec![vec![Value::Int64(0)]]);
}
