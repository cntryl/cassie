#[test]
fn should_report_relation_fork_sizes_for_a_created_table() {
    // Arrange
    let fixture = size_fixture("issue-817-relation-fork-sizes");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_relation_size('issue_817_size_probe'::regclass) \
                        AS relation_bytes, \
                    pg_catalog.pg_relation_size('issue_817_size_probe'::regclass, 'main') \
                        AS main_fork_bytes, \
                    pg_catalog.pg_relation_size('issue_817_size_probe'::regclass, 'fsm') \
                        AS free_space_map_bytes, \
                    pg_catalog.pg_relation_size('issue_817_size_probe'::regclass, 'vm') \
                        AS visibility_map_bytes",
        )
        .expect("read the created table's main, free-space, and visibility map sizes");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![
            ("relation_bytes", 20),
            ("main_fork_bytes", 20),
            ("free_space_map_bytes", 20),
            ("visibility_map_bytes", 20),
        ]
    );
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::Int64(8192),
            Value::Int64(8192),
            Value::Int64(0),
            Value::Int64(0),
        ]]
    );
}

#[test]
fn should_resolve_relation_size_names_through_search_path() {
    // Arrange
    let fixture = sql_fixture(
        "issue-817-relation-size-search-path",
        &[
            "CREATE SCHEMA issue_817_size_path",
            "CREATE TABLE issue_817_size_path.visible (id INTEGER)",
            "INSERT INTO issue_817_size_path.visible VALUES (1)",
        ],
    );
    fixture.cassie.startup().expect("startup");
    fixture
        .execute("SET search_path TO issue_817_size_path, public")
        .expect("put the fixture schema first in the session search_path");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_relation_size('visible'::regclass) AS visible_size, \
                    pg_catalog.pg_relation_size( \
                        'issue_817_size_path.visible'::regclass) AS qualified_size",
        )
        .expect("resolve unqualified and schema-qualified relation names");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![("visible_size", 20), ("qualified_size", 20)]
    );
    assert_eq!(
        selected.rows(),
        vec![vec![Value::Int64(8192), Value::Int64(8192)]]
    );
}

#[test]
fn should_report_relation_size_components() {
    // Arrange
    let fixture = size_fixture("issue-817-relation-size-functions");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_table_size('issue_817_size_probe'::regclass) \
                        AS table_bytes, \
                    pg_catalog.pg_indexes_size('issue_817_size_probe'::regclass) \
                        AS index_bytes, \
                    pg_catalog.pg_total_relation_size('issue_817_size_probe'::regclass) \
                        AS total_relation_bytes",
        )
        .expect("read table, index, and total relation sizes");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![
            ("table_bytes", 20),
            ("index_bytes", 20),
            ("total_relation_bytes", 20),
        ]
    );
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::Int64(16_384),
            Value::Int64(16_384),
            Value::Int64(32_768),
        ]]
    );
}

#[test]
fn should_report_toast_table_sizes() {
    // Arrange
    let payload = "x".repeat(16_384);
    let insert_payload = format!("INSERT INTO issue_817_toast_probe VALUES (1, '{payload}')");
    let fixture = sql_fixture(
        "issue-817-toast-relation-sizes",
        &[
            "CREATE TABLE issue_817_toast_probe (id INTEGER, payload TEXT)",
            "ALTER TABLE issue_817_toast_probe \
             ALTER COLUMN payload SET STORAGE EXTERNAL",
            &insert_payload,
        ],
    );
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_relation_size('issue_817_toast_probe'::regclass) \
                        AS main_relation_bytes, \
                    pg_catalog.pg_table_size('issue_817_toast_probe'::regclass) \
                        AS table_and_toast_bytes, \
                    pg_catalog.pg_total_relation_size('issue_817_toast_probe'::regclass) \
                        AS total_relation_bytes",
        )
        .expect("read relation sizes after storing an uncompressed out-of-line value");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![
            ("main_relation_bytes", 20),
            ("table_and_toast_bytes", 20),
            ("total_relation_bytes", 20),
        ]
    );
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::Int64(8192),
            Value::Int64(73_728),
            Value::Int64(73_728),
        ]]
    );
}

#[test]
fn should_resolve_database_size_overloads() {
    // Arrange
    let fixture = sql_fixture("issue-817-database-size-overloads", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_database_size(current_database()) AS database_size_by_name, \
                    pg_catalog.pg_database_size( \
                        (SELECT oid FROM pg_catalog.pg_database \
                         WHERE datname = current_database())) AS database_size_by_oid",
        )
        .expect("resolve the active database size through both PostgreSQL overloads");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![("database_size_by_name", 20), ("database_size_by_oid", 20)]
    );
    let [database_size, oid_size] = selected.rows()[0].as_slice() else {
        panic!("expected one row with both database size overloads");
    };
    let (Value::Int64(database_size), Value::Int64(oid_size)) = (database_size, oid_size) else {
        panic!("database size overloads must return PostgreSQL bigint values");
    };
    assert!(*database_size > 0);
    assert_eq!(database_size, oid_size);
}

#[test]
fn should_format_numeric_size_with_null_input() {
    // Arrange
    let fixture = sql_fixture("issue-817-size-pretty-numeric-overload", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_size_pretty(1048576::numeric) AS numeric_pretty, \
                    pg_catalog.pg_size_pretty(NULL::numeric) AS null_pretty",
        )
        .expect("format numeric and null byte counts");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![("numeric_pretty", 25), ("null_pretty", 25)]
    );
    assert_eq!(
        selected.rows(),
        vec![vec![Value::String("1024 kB".to_owned()), Value::Null]]
    );
}

#[test]
fn should_format_size_units_at_postgresql_boundaries() {
    // Arrange
    let fixture = sql_fixture("issue-817-size-pretty-unit-boundaries", &[]);
    fixture.cassie.startup().expect("startup");

    // Act
    let selected = fixture
        .execute(
            "SELECT pg_catalog.pg_size_pretty(10239::bigint) AS below_ten_kib, \
                    pg_catalog.pg_size_pretty(10240::bigint) AS ten_kib, \
                    pg_catalog.pg_size_pretty(-10240::bigint) AS negative_ten_kib, \
                    pg_catalog.pg_size_pretty( \
                        100000000000000000000::numeric) AS large_numeric",
        )
        .expect("format byte counts across unit boundaries and the numeric range");

    // Assert
    assert_eq!(
        selected
            .columns()
            .iter()
            .map(|column| (column.name.as_str(), column.type_oid))
            .collect::<Vec<_>>(),
        vec![
            ("below_ten_kib", 25),
            ("ten_kib", 25),
            ("negative_ten_kib", 25),
            ("large_numeric", 25),
        ]
    );
    assert_eq!(
        selected.rows(),
        vec![vec![
            Value::String("10239 bytes".to_owned()),
            Value::String("10 kB".to_owned()),
            Value::String("-10 kB".to_owned()),
            Value::String("88818 PB".to_owned()),
        ]]
    );
}
