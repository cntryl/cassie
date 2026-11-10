#[test]
fn should_report_created_table_attribute_type_identities() {
    // Arrange
    let fixture = created_object_catalog_fixture("issue-817-created-attribute-identities");

    // Act
    let attributes = fixture
        .execute(
            "SELECT attname, attnum, atttypid, atttypmod, attnotnull, attidentity::text \
             FROM pg_catalog.pg_attribute \
             WHERE attrelid = 'items' \
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

#[test]
fn should_report_postgresql_builtin_type_metadata() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-type-kind-category", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT oid, typtype, typcategory FROM pg_catalog.pg_type WHERE oid IN (16, 23, 25, 705, 1007, 2205, 3802) ORDER BY oid")
        .expect("read built-in type classification");

    // Assert
    let expected = [
        (16, "b", "B"),
        (23, "b", "N"),
        (25, "b", "S"),
        (705, "p", "X"),
        (1007, "b", "A"),
        (2205, "b", "N"),
        (3802, "b", "U"),
    ]
    .map(|(oid, kind, category)| {
        vec![
            Value::Int64(oid),
            Value::String(kind.into()),
            Value::String(category.into()),
        ]
    })
    .to_vec();
    assert_eq!(selected.rows(), expected);
}

#[test]
fn should_keep_private_vector_type_classification() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-private-vector-type-kind",
        &["CREATE TABLE vector_kind (embedding VECTOR(2))"],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute("SELECT typtype, typcategory, typbyval FROM pg_catalog.pg_type WHERE typname = 'vector(2)'")
        .expect("read private vector type");

    // Assert
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("b".into()),
            Value::String("V".into()),
            Value::Bool(false)
        ]]
    );
}

#[test]
fn should_report_builtin_type_pass_by_value_flags() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-type-pass-by-value", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture.execute("SELECT oid, typbyval FROM pg_catalog.pg_type WHERE oid IN (16,17,19,20,21,23,24,25,26,114,650,700,701,705,774,829,869,1007,1042,1043,1082,1083,1114,1184,1186,1266,1560,1562,1700,2202,2203,2204,2205,2206,2950,3220,3734,3769,3802,4072,4089,4096,4191) ORDER BY oid").expect("read builtin pass-by-value flags");

    // Assert
    let expected = [
        (16, true),
        (17, false),
        (19, false),
        (20, true),
        (21, true),
        (23, true),
        (24, true),
        (25, false),
        (26, true),
        (114, false),
        (650, false),
        (700, true),
        (701, true),
        (705, false),
        (774, false),
        (829, false),
        (869, false),
        (1007, false),
        (1042, false),
        (1043, false),
        (1082, true),
        (1083, true),
        (1114, true),
        (1184, true),
        (1186, false),
        (1266, false),
        (1560, false),
        (1562, false),
        (1700, false),
        (2202, true),
        (2203, true),
        (2204, true),
        (2205, true),
        (2206, true),
        (2950, false),
        (3220, true),
        (3734, true),
        (3769, true),
        (3802, false),
        (4072, false),
        (4089, true),
        (4096, true),
        (4191, true),
    ]
    .map(|(oid, by_value)| vec![Value::Int64(oid), Value::Bool(by_value)])
    .to_vec();
    assert_eq!(selected.rows(), expected);
}
