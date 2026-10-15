#[test]
fn should_list_created_enum_labels_with_postgresql_type_identity() {
    // Arrange
    let fixture = sql_fixture("issue-817-enum-catalog-rows", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE TYPE issue_817_catalog_state AS ENUM ('queued', 'active', 'done')")
        .expect("create the enum whose catalog rows are inspected");
    let sql = "SELECT enumlabel, \
                      enumtypid = 'issue_817_catalog_state'::regtype::oid, \
                      oid, enumsortorder::text \
               FROM pg_catalog.pg_enum \
               WHERE enumtypid = 'issue_817_catalog_state'::regtype::oid \
               ORDER BY enumsortorder";

    // Act
    let rows = fixture
        .execute(sql)
        .expect("read actual pg_enum rows for the created type")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(rows.len(), 3, "one catalog row per declared label");
    assert_eq!(
        rows.iter().map(|row| row[0].clone()).collect::<Vec<_>>(),
        vec![
            Value::String("queued".to_owned()),
            Value::String("active".to_owned()),
            Value::String("done".to_owned()),
        ]
    );
    assert!(rows.iter().all(|row| row[1] == Value::Bool(true)));
    assert_eq!(
        rows.iter().map(|row| row[3].clone()).collect::<Vec<_>>(),
        vec![
            Value::String("1".to_owned()),
            Value::String("2".to_owned()),
            Value::String("3".to_owned()),
        ]
    );
    let mut enum_oids = rows
        .iter()
        .map(|row| match &row[2] {
            Value::Int64(oid) => *oid,
            other => panic!("pg_enum.oid must be an OID value, got {other:?}"),
        })
        .collect::<Vec<_>>();
    assert!(enum_oids.iter().all(|oid| *oid > 0));
    enum_oids.sort_unstable();
    assert!(enum_oids.windows(2).all(|pair| pair[0] != pair[1]));
}

#[test]
fn should_resolve_quoted_custom_regtype_names() {
    // Arrange
    let fixture = sql_fixture("issue-818-quoted-custom-regtype", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE SCHEMA \"Issue_818_Types\"")
        .expect("create a mixed-case schema");
    fixture
        .execute(
            "CREATE TYPE \"Issue_818_Types\".\"StateCode\" \
             AS ENUM ('Ready', 'Done')",
        )
        .expect("create a mixed-case enum type");

    // Act
    let result = fixture
        .execute(
            "SELECT to_regtype('\"Issue_818_Types\".\"StateCode\"')::oid = \
                    '\"Issue_818_Types\".\"StateCode\"'::regtype::oid, \
                    to_regtype('\"Issue_818_Types\".\"MissingType\"') IS NULL, \
                    to_regtype('\"issue_818_types\".\"StateCode\"') IS NULL",
        )
        .expect("resolve the exact quoted regtype names");

    // Assert
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true)
        ]]
    );
}

#[test]
fn should_resolve_text_search_regtype_inputs() {
    // Arrange
    let fixture = sql_fixture("issue-818-text-search-reg-inputs", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let result = fixture
        .execute(
            "SELECT 'english'::regconfig::oid = 'pg_catalog.english'::regconfig::oid, \
                    'simple'::regdictionary::oid = 'pg_catalog.simple'::regdictionary::oid, \
                    'english'::regconfig::text = 'english', \
                    'simple'::regdictionary::text = 'simple', \
                    pg_typeof('english'::regconfig) = 'regconfig'::regtype, \
                    pg_typeof('simple'::regdictionary) = 'regdictionary'::regtype",
        )
        .expect("resolve text search configuration and dictionary reg* inputs");

    // Assert
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_preserve_custom_type_identity_through_pg_typeof() {
    // Arrange
    let fixture = sql_fixture("issue-818-pg-typeof-domain-identities", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE DOMAIN issue_818_type_identity AS BIGINT")
        .expect("create a user-defined domain");

    // Act
    let result = fixture
        .execute(
            "SELECT pg_typeof(1::BIGINT) = 'bigint'::regtype, \
                    pg_typeof(1::issue_818_type_identity) = 'issue_818_type_identity'::regtype, \
                    pg_typeof(ARRAY[1::issue_818_type_identity]) = \
                      'issue_818_type_identity[]'::regtype, \
                    pg_typeof(pg_typeof(1)) = 'regtype'::regtype",
        )
        .expect("preserve scalar, domain, array, and regtype identities");

    // Assert
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
        ]]
    );
}
