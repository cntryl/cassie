//! Populated PostgreSQL type and object-identity catalog contracts for #817/#818.

use crate::support_sql_fixture::sql_fixture;
use cassie::types::Value;

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
fn should_report_sequence_catalog_option_values() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-sequence-populated-row", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute(
            "CREATE SEQUENCE issue_817_catalog_sequence \
             START WITH 7 INCREMENT BY 3 MINVALUE 2 MAXVALUE 41 CACHE 5 CYCLE",
        )
        .expect("create a sequence with distinguishing catalog options");

    // Act
    let rows = fixture
        .execute(
            "SELECT seqtypid, seqstart, seqincrement, seqmax, seqmin, seqcache, seqcycle \
             FROM pg_catalog.pg_sequence \
             WHERE seqrelid = 'issue_817_catalog_sequence'::regclass",
        )
        .expect("read the created sequence's pg_sequence row")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::Int64(20),
            Value::Int64(7),
            Value::Int64(3),
            Value::Int64(41),
            Value::Int64(2),
            Value::Int64(5),
            Value::Bool(true),
        ]]
    );
}

#[test]
fn should_report_partitioned_table_catalog_metadata() {
    // Arrange
    let fixture = sql_fixture("issue-817-pg-partitioned-table-row", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute(
            "CREATE TABLE issue_817_partitioned_catalog_parent (id INTEGER, payload TEXT) \
             PARTITION BY RANGE (id)",
        )
        .expect("create a range-partitioned parent");
    fixture
        .execute(
            "CREATE TABLE issue_817_partitioned_catalog_child \
             PARTITION OF issue_817_partitioned_catalog_parent \
             FOR VALUES FROM (0) TO (10)",
        )
        .expect("create a valid range partition");
    let parent_kind = fixture
        .execute(
            "SELECT relkind::text FROM pg_catalog.pg_class \
             WHERE oid = 'issue_817_partitioned_catalog_parent'::regclass",
        )
        .expect("confirm the partitioned parent exists in pg_class");
    assert_eq!(parent_kind.rows(), &[vec![Value::String("p".to_owned())]]);

    // Act
    let rows = fixture
        .execute(
            "SELECT partstrat::text, partnatts, partdefid, partattrs::text \
             FROM pg_catalog.pg_partitioned_table \
             WHERE partrelid = 'issue_817_partitioned_catalog_parent'::regclass",
        )
        .expect("read the partition strategy and key from pg_partitioned_table")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::String("r".to_owned()),
            Value::Int64(1),
            Value::Int64(0),
            Value::String("1".to_owned()),
        ]]
    );
}

#[test]
fn should_expose_postgresql_system_columns_with_declared_identities() {
    // Arrange
    let fixture = sql_fixture("issue-817-system-column-identities", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE TABLE issue_817_system_column_row (id INTEGER PRIMARY KEY)")
        .expect("create a table with a physical row");
    fixture
        .execute("INSERT INTO issue_817_system_column_row VALUES (1)")
        .expect("insert the row whose system columns are inspected");
    let sql = "SELECT pg_typeof(ctid) = 'tid'::regtype, \
                      pg_typeof(xmin) = 'xid'::regtype, \
                      pg_typeof(xmax) = 'xid'::regtype, \
                      pg_typeof(cmin) = 'cid'::regtype, \
                      pg_typeof(cmax) = 'cid'::regtype, \
                      pg_typeof(tableoid) = 'oid'::regtype, \
                      tableoid = 'issue_817_system_column_row'::regclass, \
                      ctid IS NOT NULL, xmin::text <> '0', xmax::text = '0' \
               FROM issue_817_system_column_row";

    // Act
    let rows = fixture
        .execute(sql)
        .expect("read the six PostgreSQL system columns from the stored row")
        .rows()
        .to_vec();

    // Assert
    assert_eq!(
        rows,
        vec![vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
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
fn should_resolve_user_function_overloads_through_to_regprocedure() {
    // Arrange
    let fixture = sql_fixture("issue-818-user-regprocedure-overloads", &[]);
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("CREATE SCHEMA issue_818_routine_lookup")
        .expect("create the overload lookup schema");
    fixture
        .execute(
            "CREATE FUNCTION issue_818_routine_lookup.resolve(INTEGER) \
             RETURNS INTEGER LANGUAGE SQL IMMUTABLE AS $$ SELECT 1 $$",
        )
        .expect("create the integer overload");
    fixture
        .execute(
            "CREATE FUNCTION issue_818_routine_lookup.resolve(TEXT) \
             RETURNS TEXT LANGUAGE SQL IMMUTABLE AS $$ SELECT 'x' $$",
        )
        .expect("create the text overload");
    fixture
        .execute("SET search_path TO issue_818_routine_lookup, public")
        .expect("put the custom overloads on search_path");

    // Act
    let result = fixture
        .execute(
            "SELECT to_regprocedure('resolve(integer)')::oid = \
                    to_regprocedure('resolve(int4)')::oid, \
                    to_regprocedure('resolve(integer)')::oid <> \
                    to_regprocedure('resolve(text)')::oid, \
                    to_regprocedure('resolve(bigint)') IS NULL, \
                    to_regprocedure('resolve(integer)')::text = 'resolve(integer)', \
                    to_regprocedure('resolve(text)')::text = 'resolve(text)'",
        )
        .expect("resolve each custom routine signature through search_path");

    // Assert
    assert_eq!(
        result.rows(),
        &[vec![
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
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
    assert!(transaction_old_name.is_empty());
    assert_eq!(
        after_rollback,
        vec![expected_row("before_kind", &initial_oid)]
    );
    assert!(rollback_name_absent.is_empty());
    assert_eq!(
        after_rename,
        vec![expected_row("persisted_kind", &initial_oid)]
    );
    assert!(old_name_absent.is_empty());
    assert!(after_drop.is_empty());
    assert_eq!(dropped_relation, vec![vec![Value::Int64(0)]]);
}
