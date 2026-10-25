#[test]
fn should_report_created_table_attribute_type_identities() {
    // Arrange
    let fixture = created_object_catalog_fixture("issue-817-created-attribute-identities");

    // Act
    let attributes = fixture
        .execute(
            "SELECT attname, attnum, atttypid, atttypmod, attnotnull, attidentity::text \
             FROM pg_catalog.pg_attribute \
             WHERE attrelid = 'issue_817_catalog_probe.items'::regclass \
               AND attnum > 0 \
             ORDER BY attnum",
        )
        .expect("read table attribute type identities");

    // Assert
    assert_eq!(
        attributes.rows(),
        vec![
            vec![
                Value::String("id".to_owned()),
                Value::Int64(1),
                Value::Int64(23),
                Value::Int64(-1),
                Value::Bool(true),
                Value::String(String::new()),
            ],
            vec![
                Value::String("amount".to_owned()),
                Value::Int64(2),
                Value::Int64(1_700),
                Value::Int64(-1),
                Value::Bool(true),
                Value::String(String::new()),
            ],
        ]
    );
}

#[test]
fn should_report_underlying_type_identity_in_information_schema_columns() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-information-schema-column-type-identity",
        &["CREATE TABLE issue_817_type_identity_columns (\
            integer_value INTEGER, \
            text_value TEXT, \
            varchar_value VARCHAR(12))"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT column_name, udt_catalog, udt_schema, udt_name \
             FROM information_schema.columns \
             WHERE table_schema = 'public' \
               AND table_name = 'issue_817_type_identity_columns' \
             ORDER BY ordinal_position",
        )
        .expect("read underlying type identities");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![
            vec![
                Value::String("integer_value".to_owned()),
                Value::String("postgres".to_owned()),
                Value::String("pg_catalog".to_owned()),
                Value::String("int4".to_owned()),
            ],
            vec![
                Value::String("text_value".to_owned()),
                Value::String("postgres".to_owned()),
                Value::String("pg_catalog".to_owned()),
                Value::String("text".to_owned()),
            ],
            vec![
                Value::String("varchar_value".to_owned()),
                Value::String("postgres".to_owned()),
                Value::String("pg_catalog".to_owned()),
                Value::String("varchar".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_scope_underlying_type_catalogs_given_same_named_tables_and_views_when_database_sessions_query_columns(
) {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-underlying-type-database-scope",
        &[
            "CREATE DATABASE issue_817_type_tenant",
            "CREATE TABLE issue_817_type_scope (id INTEGER)",
            "CREATE VIEW issue_817_type_scope_view AS SELECT id FROM issue_817_type_scope",
        ],
    );
    fixture.cassie.startup().expect("startup");
    let tenant = fixture
        .cassie
        .create_session("tester", Some("issue_817_type_tenant".to_owned()));
    for sql in [
        "CREATE TABLE issue_817_type_scope (title VARCHAR(12))",
        "CREATE VIEW issue_817_type_scope_view AS SELECT title FROM issue_817_type_scope",
    ] {
        fixture
            .cassie
            .execute_sql(&tenant, sql, vec![])
            .expect("create same-named tenant objects with a distinct column type");
    }
    let sql = "SELECT table_name, column_name, udt_catalog, udt_schema, udt_name \
               FROM information_schema.columns \
               WHERE table_name IN ('issue_817_type_scope', 'issue_817_type_scope_view') \
               ORDER BY table_name, ordinal_position";

    // Act
    let default_columns = fixture.execute(sql).expect("read default database columns");
    let tenant_columns = fixture
        .cassie
        .execute_sql(&tenant, sql, vec![])
        .expect("read tenant database columns");

    // Assert
    assert_eq!(
        default_columns.rows(),
        vec![
            vec![
                Value::String("issue_817_type_scope".to_owned()),
                Value::String("id".to_owned()),
                Value::String("postgres".to_owned()),
                Value::String("pg_catalog".to_owned()),
                Value::String("int4".to_owned()),
            ],
            vec![
                Value::String("issue_817_type_scope_view".to_owned()),
                Value::String("id".to_owned()),
                Value::String("postgres".to_owned()),
                Value::String("pg_catalog".to_owned()),
                Value::String("int4".to_owned()),
            ],
        ],
    );
    assert_eq!(
        tenant_columns.rows(),
        vec![
            vec![
                Value::String("issue_817_type_scope".to_owned()),
                Value::String("title".to_owned()),
                Value::String("issue_817_type_tenant".to_owned()),
                Value::String("pg_catalog".to_owned()),
                Value::String("varchar".to_owned()),
            ],
            vec![
                Value::String("issue_817_type_scope_view".to_owned()),
                Value::String("title".to_owned()),
                Value::String("issue_817_type_tenant".to_owned()),
                Value::String("pg_catalog".to_owned()),
                Value::String("varchar".to_owned()),
            ],
        ],
    );
}

#[test]
fn should_report_no_builtin_enum_labels_without_user_types() {
    // Arrange
    let fixture = sql_fixture("issue-817-enum-catalog", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT count(*) FROM pg_catalog.pg_enum")
        .expect("query the PostgreSQL enum catalog");

    // Assert
    assert_eq!(selected.rows(), vec![vec![Value::Int64(0)]]);
}

#[test]
fn should_report_builtin_int4_array_type_links_in_pg_type() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-type-array-links", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT typname, oid, typelem, typarray \
             FROM pg_catalog.pg_type \
             WHERE typname IN ('int4', '_int4') \
             ORDER BY oid",
        )
        .expect("read the scalar and array type identities");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![
            vec![
                Value::String("int4".to_owned()),
                Value::Int64(23),
                Value::Int64(0),
                Value::Int64(1_007),
            ],
            vec![
                Value::String("_int4".to_owned()),
                Value::Int64(1_007),
                Value::Int64(23),
                Value::Int64(0),
            ],
        ]
    );
}
