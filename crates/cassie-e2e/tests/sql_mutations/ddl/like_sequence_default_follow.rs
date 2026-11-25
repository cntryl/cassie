//! LIKE-copied sequence defaults follow their sequence and column storage
//! metadata follows column type changes (#826).
use cassie::app::{Cassie, CassieSession};
use cassie::types::Value;

fn start() -> (Cassie, CassieSession) {
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    (cassie, session)
}

fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("execute {sql}: {error}"));
}

fn failure(cassie: &Cassie, session: &CassieSession, sql: &str) -> String {
    match cassie.execute_sql(session, sql, vec![]) {
        Ok(_) => panic!("expected {sql} to fail"),
        Err(error) => error.to_string(),
    }
}

fn first_id(cassie: &Cassie, session: &CassieSession, sql: &str) -> i64 {
    let result = cassie
        .execute_sql(session, sql, vec![])
        .unwrap_or_else(|error| panic!("execute {sql}: {error}"));
    match result.rows() {
        [row] => match row.as_slice() {
            [Value::Int64(value)] => *value,
            _ => panic!("unexpected row shape for {sql}"),
        },
        _ => panic!("expected one row for {sql}"),
    }
}

fn setup_clone(cassie: &Cassie, session: &CassieSession) {
    execute(cassie, session, "CREATE TABLE seq_src (id serial)");
    execute(
        cassie,
        session,
        "CREATE TABLE seq_dep (LIKE seq_src INCLUDING DEFAULTS)",
    );
}

fn storage_of(cassie: &Cassie, session: &CassieSession, table: &str) -> Vec<(String, String)> {
    let result = cassie
        .execute_sql(
            session,
            &format!(
                "SELECT attname::TEXT, attstorage::TEXT || '/' || attcompression::TEXT \
                 FROM pg_catalog.pg_attribute \
                 WHERE attrelid = '{table}'::regclass AND attnum > 0 AND NOT attisdropped \
                 ORDER BY attnum"
            ),
            vec![],
        )
        .expect("read storage metadata");
    result
        .rows()
        .iter()
        .map(|row| match row.as_slice() {
            [Value::String(name), Value::String(storage)] => (name.clone(), storage.clone()),
            _ => panic!("unexpected storage row"),
        })
        .collect()
}

#[test]
fn should_keep_copied_default_working_after_source_table_rename() {
    // Arrange
    let (cassie, session) = start();
    setup_clone(&cassie, &session);

    // Act
    execute(&cassie, &session, "ALTER TABLE seq_src RENAME TO seq_src2");
    let inserted = first_id(
        &cassie,
        &session,
        "INSERT INTO seq_dep DEFAULT VALUES RETURNING id",
    );
    let blocked = failure(&cassie, &session, "DROP TABLE seq_src2");

    // Assert
    assert_eq!(inserted, 1);
    assert!(
        blocked.contains("depend"),
        "unexpected drop result {blocked}"
    );
}

#[test]
fn should_keep_copied_default_working_after_source_table_schema_move() {
    // Arrange
    let (cassie, session) = start();
    setup_clone(&cassie, &session);
    execute(&cassie, &session, "CREATE SCHEMA seq_moved");

    // Act
    execute(
        &cassie,
        &session,
        "ALTER TABLE seq_src SET SCHEMA seq_moved",
    );
    let inserted = first_id(
        &cassie,
        &session,
        "INSERT INTO seq_dep DEFAULT VALUES RETURNING id",
    );
    let blocked = failure(&cassie, &session, "DROP TABLE seq_moved.seq_src");

    // Assert
    assert_eq!(inserted, 1);
    assert!(
        blocked.contains("depend"),
        "unexpected drop result {blocked}"
    );
}

#[test]
fn should_restore_copied_default_when_rename_transaction_rolls_back() {
    // Arrange
    let (cassie, session) = start();
    setup_clone(&cassie, &session);

    // Act
    execute(&cassie, &session, "BEGIN");
    execute(&cassie, &session, "ALTER TABLE seq_src RENAME TO seq_src2");
    execute(&cassie, &session, "ROLLBACK");
    let inserted = first_id(
        &cassie,
        &session,
        "INSERT INTO seq_dep DEFAULT VALUES RETURNING id",
    );
    let blocked = failure(&cassie, &session, "DROP TABLE seq_src");

    // Assert
    assert_eq!(inserted, 1);
    assert!(
        blocked.contains("depend"),
        "unexpected drop result {blocked}"
    );
}

#[test]
fn should_refuse_dropping_sequence_used_by_explicit_nextval_default() {
    // Arrange
    let (cassie, session) = start();
    execute(&cassie, &session, "CREATE SEQUENCE seq_explicit");
    execute(
        &cassie,
        &session,
        "CREATE TABLE seq_explicit_user (id BIGINT DEFAULT nextval('seq_explicit'))",
    );
    execute(
        &cassie,
        &session,
        "CREATE TABLE seq_altered_user (id BIGINT)",
    );
    execute(
        &cassie,
        &session,
        "ALTER TABLE seq_altered_user ALTER COLUMN id SET DEFAULT nextval('seq_explicit')",
    );
    execute(&cassie, &session, "DROP TABLE seq_explicit_user");

    // Act
    let blocked = failure(&cassie, &session, "DROP SEQUENCE seq_explicit");

    // Assert
    assert!(
        blocked.contains("depend"),
        "unexpected drop result {blocked}"
    );
}

#[test]
fn should_drop_source_with_dependent_tables_in_one_statement() {
    // Arrange
    let (cassie, session) = start();
    setup_clone(&cassie, &session);

    // Act
    execute(&cassie, &session, "DROP TABLE seq_src, seq_dep");

    // Assert
    let missing = failure(&cassie, &session, "SELECT * FROM seq_dep");
    assert!(missing.contains("seq_dep"), "unexpected result {missing}");
}

#[test]
fn should_reset_column_storage_when_column_type_changes() {
    // Arrange
    let (cassie, session) = start();
    execute(
        &cassie,
        &session,
        "CREATE TABLE storage_alter (a TEXT, b TEXT, c TEXT)",
    );
    for column in ["a", "b", "c"] {
        execute(
            &cassie,
            &session,
            &format!("ALTER TABLE storage_alter ALTER COLUMN {column} SET STORAGE EXTERNAL"),
        );
        execute(
            &cassie,
            &session,
            &format!("ALTER TABLE storage_alter ALTER COLUMN {column} SET COMPRESSION lz4"),
        );
    }

    // Act
    execute(
        &cassie,
        &session,
        "ALTER TABLE storage_alter ALTER COLUMN a TYPE INT USING length(a)",
    );
    execute(
        &cassie,
        &session,
        "ALTER TABLE storage_alter ALTER COLUMN b TYPE VARCHAR USING b",
    );
    let after = storage_of(&cassie, &session, "storage_alter");
    execute(
        &cassie,
        &session,
        "CREATE TABLE storage_alter_like (LIKE storage_alter INCLUDING STORAGE INCLUDING COMPRESSION)",
    );
    let copied = storage_of(&cassie, &session, "storage_alter_like");

    // Assert
    assert_eq!(after[0], ("a".to_owned(), "p/".to_owned()));
    assert_eq!(after[1], ("b".to_owned(), "x/".to_owned()));
    assert_eq!(after[2], ("c".to_owned(), "e/l".to_owned()));
    assert_eq!(copied[..2], after[..2]);
}

#[test]
fn should_not_inherit_storage_when_dropped_column_name_is_added_again() {
    // Arrange
    let (cassie, session) = start();
    execute(
        &cassie,
        &session,
        "CREATE TABLE storage_readd (a INT, b TEXT)",
    );
    execute(
        &cassie,
        &session,
        "ALTER TABLE storage_readd ALTER COLUMN b SET STORAGE EXTERNAL",
    );
    execute(
        &cassie,
        &session,
        "ALTER TABLE storage_readd ALTER COLUMN b SET COMPRESSION lz4",
    );

    // Act
    execute(&cassie, &session, "ALTER TABLE storage_readd DROP COLUMN b");
    execute(
        &cassie,
        &session,
        "ALTER TABLE storage_readd ADD COLUMN b TEXT",
    );
    let after = storage_of(&cassie, &session, "storage_readd");

    // Assert
    assert_eq!(after[1], ("b".to_owned(), "x/".to_owned()));
}

#[test]
fn should_keep_copied_default_working_after_rename_with_local_restart() {
    // Arrange
    let directory = crate::support_catalog_directory::CatalogDirectory::new("like-seq-rename");
    {
        let cassie = Cassie::new_with_data_dir_and_config(
            &directory.0,
            cassie::config::CassieRuntimeConfig::default(),
        )
        .expect("create local-storage Cassie");
        cassie.startup().expect("start local-storage Cassie");
        let session = cassie.create_session("tester", None);
        setup_clone(&cassie, &session);
        execute(&cassie, &session, "ALTER TABLE seq_src RENAME TO seq_src2");
    }

    // Act
    let cassie = Cassie::new_with_data_dir_and_config(
        &directory.0,
        cassie::config::CassieRuntimeConfig::default(),
    )
    .expect("reopen local-storage Cassie");
    cassie.startup().expect("recover local-storage Cassie");
    let session = cassie.create_session("tester", None);
    let inserted = first_id(
        &cassie,
        &session,
        "INSERT INTO seq_dep DEFAULT VALUES RETURNING id",
    );
    let blocked = failure(&cassie, &session, "DROP TABLE seq_src2");

    // Assert
    assert_eq!(inserted, 1);
    assert!(
        blocked.contains("depend"),
        "unexpected drop result {blocked}"
    );
}
