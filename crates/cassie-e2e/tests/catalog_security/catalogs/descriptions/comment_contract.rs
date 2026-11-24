//! Selected SQL object comments and description catalogs for #845.

use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::config::CassieRuntimeConfig;
use cassie::types::Value;
use std::path::PathBuf;

use crate::support_sql_fixture::sql_fixture;

fn selected_descriptions(cassie: &Cassie, session: &CassieSession) -> Vec<Vec<Value>> {
    cassie
        .execute_sql(
            session,
            "SELECT obj_description('issue_845_schema'::regnamespace::oid, 'pg_namespace'), \
                    obj_description('issue_845_schema.comment_subject'::regclass::oid, 'pg_class'), \
                    col_description('issue_845_schema.comment_subject'::regclass::oid, 1)",
            vec![],
        )
        .expect("read selected object comments")
        .rows()
        .to_vec()
}

fn scalar_i64(cassie: &Cassie, session: &CassieSession, sql: &str) -> i64 {
    match &cassie
        .execute_sql(session, sql, vec![])
        .expect("read object address")
        .rows()[0][0]
    {
        Value::Int64(value) => *value,
        value => panic!("expected integer object address, got {value:?}"),
    }
}

#[test]
fn should_return_comments_from_selected_catalog_descriptors() {
    // Arrange
    let fixture = sql_fixture(
        "issue-845-comment-values",
        &[
            "CREATE SCHEMA issue_845_schema",
            "CREATE TABLE issue_845_schema.comment_subject (id INTEGER, payload TEXT)",
        ],
    );
    fixture
        .execute("COMMENT ON SCHEMA issue_845_schema IS 'schema comment'")
        .expect("comment on schema");
    fixture
        .execute("COMMENT ON TABLE issue_845_schema.comment_subject IS 'table comment'")
        .expect("comment on table");
    fixture
        .execute("COMMENT ON COLUMN issue_845_schema.comment_subject.payload IS 'column comment'")
        .expect("comment on column");

    // Act
    let descriptions = fixture
        .execute(
            "SELECT obj_description('issue_845_schema'::regnamespace::oid, 'pg_namespace'), \
                    obj_description('issue_845_schema.comment_subject'::regclass::oid, 'pg_class'), \
                    col_description('issue_845_schema.comment_subject'::regclass::oid, 2)",
        )
        .expect("read comments through description functions");
    let catalog = fixture
        .execute(
            "SELECT classoid, objsubid, description \
             FROM pg_catalog.pg_description \
             WHERE description IN ('schema comment', 'table comment', 'column comment') \
             ORDER BY classoid, objsubid",
        )
        .expect("read the finite pg_description rows");

    // Assert
    assert_eq!(
        descriptions.rows(),
        vec![vec![
            Value::String("schema comment".to_owned()),
            Value::String("table comment".to_owned()),
            Value::String("column comment".to_owned()),
        ]]
    );
    assert_eq!(
        descriptions
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![25, 25, 25]
    );
    assert_eq!(
        catalog
            .columns()
            .iter()
            .map(|column| column.type_oid)
            .collect::<Vec<_>>(),
        vec![26, 23, 25]
    );
    assert_eq!(
        catalog.rows(),
        vec![
            vec![
                Value::Int64(1259),
                Value::Int64(0),
                Value::String("table comment".to_owned()),
            ],
            vec![
                Value::Int64(1259),
                Value::Int64(2),
                Value::String("column comment".to_owned()),
            ],
            vec![
                Value::Int64(2615),
                Value::Int64(0),
                Value::String("schema comment".to_owned()),
            ],
        ]
    );
}

#[test]
fn should_resolve_single_argument_object_descriptions_for_relation_oids() {
    // Arrange
    let fixture = sql_fixture(
        "issue-845-single-argument-description",
        &[
            "CREATE SCHEMA issue_845_single_schema",
            "CREATE TABLE issue_845_single_schema.subject (id INTEGER)",
        ],
    );
    fixture
        .execute("COMMENT ON SCHEMA issue_845_single_schema IS 'schema note'")
        .expect("comment on schema");
    fixture
        .execute("COMMENT ON TABLE issue_845_single_schema.subject IS 'table note'")
        .expect("comment on table");

    // Act
    let result = fixture
        .execute(
            "SELECT obj_description('issue_845_single_schema'::regnamespace::oid), \
                    obj_description('issue_845_single_schema.subject'::regclass::oid)",
        )
        .expect("read one-argument descriptions");

    // Assert
    assert_eq!(
        result.rows(),
        vec![vec![
            Value::String("schema note".to_owned()),
            Value::String("table note".to_owned()),
        ]]
    );
}

#[test]
fn should_scope_comment_reads_to_the_current_database_family() {
    // Arrange
    let fixture = sql_fixture(
        "issue-845-database-scope",
        &["CREATE TABLE issue_845_scoped_subject (id INTEGER)"],
    );
    fixture
        .execute("COMMENT ON TABLE issue_845_scoped_subject IS 'default database note'")
        .expect("comment on default database table");
    let default_oid = scalar_i64(
        &fixture.cassie,
        &fixture.session,
        "SELECT 'issue_845_scoped_subject'::regclass::oid::bigint",
    );
    fixture
        .execute("CREATE DATABASE issue_845_other_database")
        .expect("create a second logical database");
    let other = fixture
        .cassie
        .create_session("tester", Some("issue_845_other_database".to_owned()));
    fixture
        .cassie
        .execute_sql(
            &other,
            "CREATE TABLE issue_845_scoped_subject (id INTEGER)",
            vec![],
        )
        .expect("create same-named table in second database");

    // Act
    let result = fixture
        .cassie
        .execute_sql(
            &other,
            &format!(
                "SELECT obj_description({default_oid}::oid, 'pg_class') IS NULL, \
                        count(*) FROM pg_catalog.pg_description WHERE objoid = {default_oid}"
            ),
            vec![],
        )
        .expect("read descriptions in the second database");

    // Assert
    assert_eq!(
        result.rows(),
        vec![vec![Value::Bool(true), Value::Int64(0)]]
    );
}

#[test]
fn should_remove_comments_given_null_or_empty_string() {
    // Arrange
    let fixture = sql_fixture(
        "issue-845-comment-removal",
        &["CREATE TABLE issue_845_clear_subject (id INTEGER)"],
    );
    fixture
        .execute("COMMENT ON TABLE issue_845_clear_subject IS 'temporary'")
        .expect("set table comment");

    // Act
    fixture
        .execute("COMMENT ON TABLE issue_845_clear_subject IS ''")
        .expect("empty string removes the comment");
    let empty_cleared = fixture
        .execute(
            "SELECT obj_description('issue_845_clear_subject'::regclass::oid, 'pg_class') IS NULL, \
                    count(*) FROM pg_catalog.pg_description \
             WHERE objoid = 'issue_845_clear_subject'::regclass::oid",
        )
        .expect("verify empty-string removal");
    fixture
        .execute("COMMENT ON TABLE issue_845_clear_subject IS 'temporary again'")
        .expect("restore table comment before NULL clear");
    fixture
        .execute("COMMENT ON TABLE issue_845_clear_subject IS NULL")
        .expect("NULL removes the comment");
    let null_cleared = fixture
        .execute(
            "SELECT obj_description('issue_845_clear_subject'::regclass::oid, 'pg_class') IS NULL, \
                    count(*) FROM pg_catalog.pg_description \
             WHERE objoid = 'issue_845_clear_subject'::regclass::oid",
        )
        .expect("verify NULL removal");

    // Assert
    assert_eq!(
        empty_cleared.rows(),
        vec![vec![Value::Bool(true), Value::Int64(0)]]
    );
    assert_eq!(
        null_cleared.rows(),
        vec![vec![Value::Bool(true), Value::Int64(0)]]
    );
}

const TABLE_COMMENT_SQL: &str =
    "SELECT obj_description('issue_845_tx_subject'::regclass::oid, 'pg_class')";

#[test]
fn should_roll_back_staged_comment_changes() {
    // Arrange
    let fixture = sql_fixture(
        "issue-845-comment-rollback",
        &["CREATE TABLE issue_845_tx_subject (id INTEGER)"],
    );
    fixture
        .execute("COMMENT ON TABLE issue_845_tx_subject IS 'committed base'")
        .expect("publish base comment");
    let observer = fixture.cassie.create_session("tester", None);
    let read_observer = || {
        fixture
            .cassie
            .execute_sql(&observer, TABLE_COMMENT_SQL, vec![])
    };

    // Act
    fixture.execute("BEGIN").expect("begin transaction");
    fixture
        .execute("COMMENT ON TABLE issue_845_tx_subject IS 'transaction value'")
        .expect("stage transaction comment");
    let own_value = fixture
        .execute(TABLE_COMMENT_SQL)
        .expect("read the transaction's comment");
    let other_value =
        read_observer().expect("read only the committed comment from another session");
    fixture
        .execute("SAVEPOINT issue_845_comment_savepoint")
        .expect("create comment savepoint");
    fixture
        .execute("COMMENT ON TABLE issue_845_tx_subject IS 'savepoint value'")
        .expect("stage savepoint comment");
    fixture
        .execute("ROLLBACK TO SAVEPOINT issue_845_comment_savepoint")
        .expect("discard savepoint comment");
    let after_savepoint = fixture
        .execute(TABLE_COMMENT_SQL)
        .expect("read comment after savepoint rollback");
    fixture.execute("ROLLBACK").expect("roll back transaction");
    let after_transaction = fixture
        .execute(TABLE_COMMENT_SQL)
        .expect("read committed comment after transaction rollback");

    fixture.execute("BEGIN").expect("begin commit transaction");
    fixture
        .execute("COMMENT ON TABLE issue_845_tx_subject IS 'committed transaction value'")
        .expect("stage a transaction comment for commit");
    let own_catalog_value = fixture
        .execute(
            "SELECT description FROM pg_catalog.pg_description \
             WHERE objoid = 'issue_845_tx_subject'::regclass::oid \
             AND classoid = 'pg_class'::regclass::oid",
        )
        .expect("read staged comment from pg_description");
    let other_before_commit =
        read_observer().expect("observer sees only committed data before commit");
    fixture
        .execute("COMMIT")
        .expect("commit transaction comment");
    let other_after_commit = read_observer().expect("observer sees committed comment");

    // Assert
    assert_eq!(own_value.rows(), comment_rows("transaction value"));
    assert_eq!(other_value.rows(), comment_rows("committed base"));
    assert_eq!(after_savepoint.rows(), comment_rows("transaction value"));
    assert_eq!(after_transaction.rows(), comment_rows("committed base"));
    assert_eq!(
        own_catalog_value.rows(),
        comment_rows("committed transaction value")
    );
    assert_eq!(other_before_commit.rows(), comment_rows("committed base"));
    assert_eq!(
        other_after_commit.rows(),
        comment_rows("committed transaction value")
    );
}

#[test]
fn should_track_comment_addresses_through_object_lifecycle() {
    // Arrange
    let fixture = sql_fixture(
        "issue-845-comment-renames",
        &[
            "CREATE SCHEMA issue_845_before",
            "CREATE TABLE issue_845_before.comment_subject (id INTEGER, payload TEXT)",
        ],
    );
    fixture
        .execute("COMMENT ON SCHEMA issue_845_before IS 'schema survives rename'")
        .expect("comment on schema");
    fixture
        .execute("COMMENT ON TABLE issue_845_before.comment_subject IS 'table survives rename'")
        .expect("comment on table");
    fixture
        .execute(
            "COMMENT ON COLUMN issue_845_before.comment_subject.payload IS 'column survives rename'",
        )
        .expect("comment on column");
    let relation_oid = scalar_i64(
        &fixture.cassie,
        &fixture.session,
        "SELECT 'issue_845_before.comment_subject'::regclass::oid::bigint",
    );
    let schema_oid = scalar_i64(
        &fixture.cassie,
        &fixture.session,
        "SELECT 'issue_845_before'::regnamespace::oid::bigint",
    );

    // Act
    fixture
        .execute("ALTER SCHEMA issue_845_before RENAME TO issue_845_after")
        .expect("rename schema without changing its identity");
    fixture
        .execute("ALTER TABLE issue_845_after.comment_subject RENAME TO renamed_subject")
        .expect("rename table without changing its identity");
    fixture
        .execute("ALTER TABLE issue_845_after.renamed_subject RENAME COLUMN payload TO body")
        .expect("rename column without changing its attribute identity");
    let renamed_descriptions = fixture
        .execute(
            "SELECT obj_description('issue_845_after'::regnamespace::oid, 'pg_namespace'), \
                    obj_description('issue_845_after.renamed_subject'::regclass::oid, 'pg_class'), \
                    col_description('issue_845_after.renamed_subject'::regclass::oid, 2)",
        )
        .expect("read comments after rename");
    fixture
        .execute("ALTER TABLE issue_845_after.renamed_subject DROP COLUMN body")
        .expect("drop the commented field");
    let after_column_drop = fixture
        .execute(&format!(
            "SELECT col_description({relation_oid}::oid, 2) IS NULL, \
                    count(*) FROM pg_catalog.pg_description \
             WHERE objoid = {relation_oid} AND objsubid = 2"
        ))
        .expect("read comment cleanup after column drop");
    fixture
        .execute("DROP TABLE issue_845_after.renamed_subject")
        .expect("drop owning table");
    let after_table_drop = fixture
        .execute(&format!(
            "SELECT obj_description({relation_oid}::oid, 'pg_class') IS NULL, \
                    count(*) FROM pg_catalog.pg_description WHERE objoid = {relation_oid}"
        ))
        .expect("read comment records after table drop");
    fixture
        .execute("DROP SCHEMA issue_845_after")
        .expect("drop owning schema");
    let after_schema_drop = fixture
        .execute(&format!(
            "SELECT obj_description({schema_oid}::oid, 'pg_namespace') IS NULL, \
                    count(*) FROM pg_catalog.pg_description WHERE objoid = {schema_oid}"
        ))
        .expect("read comment records after schema drop");

    // Assert
    assert_eq!(
        renamed_descriptions.rows(),
        vec![vec![
            Value::String("schema survives rename".to_owned()),
            Value::String("table survives rename".to_owned()),
            Value::String("column survives rename".to_owned()),
        ]]
    );
    assert_eq!(
        after_column_drop.rows(),
        vec![vec![Value::Bool(true), Value::Int64(0)]]
    );
    assert_eq!(
        after_table_drop.rows(),
        vec![vec![Value::Bool(true), Value::Int64(0)]]
    );
    assert_eq!(
        after_schema_drop.rows(),
        vec![vec![Value::Bool(true), Value::Int64(0)]]
    );
}

#[test]
fn should_preserve_comment_records_across_persistence_boundaries() {
    // Arrange
    let path = data_dir("issue-845-comment-recovery");
    let directory = DataDirectory(path.clone());
    let cassie = Cassie::new_with_data_dir(&path).expect("create local database");
    cassie.startup().expect("start local database");
    let postgres = cassie.create_session("tester", Some("postgres".to_owned()));
    cassie
        .execute_sql(
            &postgres,
            "CREATE DATABASE issue_845_comment_source",
            vec![],
        )
        .expect("create image source database");
    let source = cassie.create_session("tester", Some("issue_845_comment_source".to_owned()));
    for sql in [
        "CREATE SCHEMA issue_845_schema",
        "CREATE TABLE issue_845_schema.comment_subject (payload TEXT)",
        "COMMENT ON SCHEMA issue_845_schema IS 'recovered schema'",
        "COMMENT ON TABLE issue_845_schema.comment_subject IS 'recovered table'",
        "COMMENT ON COLUMN issue_845_schema.comment_subject.payload IS 'recovered column'",
    ] {
        cassie
            .execute_sql(&source, sql, vec![])
            .unwrap_or_else(|error| panic!("execute {sql}: {error}"));
    }
    let mut backup = cassie
        .begin_database_backup("issue_845_comment_source")
        .expect("begin source database image");
    let mut chunks = Vec::new();
    while let Some(chunk) = backup.next_chunk().expect("read image chunk") {
        chunks.push(chunk);
    }

    // Act
    let mut restore = cassie
        .begin_database_restore("issue_845_comment_restored")
        .expect("begin database image restore");
    for chunk in chunks {
        restore.push_chunk(&chunk).expect("write image chunk");
    }
    restore.finish().expect("publish restored database");
    cassie.hydrate_catalog().expect("hydrate restored catalog");
    let restored = cassie.create_session("tester", Some("issue_845_comment_restored".to_owned()));
    let restored_before_restart = selected_descriptions(&cassie, &restored);
    cassie.shutdown();
    // The image streams hold Cassie clones, and so the storage writer lease.
    drop(backup);
    drop(restore);
    drop(restored);
    drop(source);
    drop(postgres);
    drop(cassie);

    let reopened = Cassie::new_with_data_dir(&path).expect("reopen local database");
    reopened.startup().expect("recover local database");
    let restored = reopened.create_session("tester", Some("issue_845_comment_restored".to_owned()));
    let restored_after_restart = selected_descriptions(&reopened, &restored);

    // Assert
    let expected = vec![vec![
        Value::String("recovered schema".to_owned()),
        Value::String("recovered table".to_owned()),
        Value::String("recovered column".to_owned()),
    ]];
    assert_eq!(restored_before_restart, expected);
    assert_eq!(restored_after_restart, expected);
    drop(restored);
    drop(reopened);
    drop(directory);
}

#[test]
fn should_reject_comment_changes_outside_the_selected_contract() {
    // Arrange
    let password = crate::support_sql::test_root_password();
    let config = CassieRuntimeConfig {
        password: password.to_owned(),
        ..CassieRuntimeConfig::default()
    };
    let cassie = Cassie::new_in_memory_with_config(config).expect("create Cassie");
    cassie.startup().expect("startup");
    let root = cassie
        .authenticate_role("root", Some(password), None)
        .expect("authenticate root");
    for sql in [
        "CREATE TABLE issue_845_auth_subject (payload TEXT)",
        "CREATE VIEW issue_845_auth_view AS SELECT payload FROM issue_845_auth_subject",
        "CREATE MATERIALIZED VIEW issue_845_auth_materialized AS SELECT payload FROM issue_845_auth_subject",
        "CREATE ROLE issue_845_reader LOGIN PASSWORD 'reader-secret'",
    ] {
        cassie
            .execute_sql(&root, sql, vec![])
            .unwrap_or_else(|error| panic!("execute {sql}: {error}"));
    }
    let reader = cassie
        .authenticate_role("issue_845_reader", Some("reader-secret"), None)
        .expect("authenticate read-only role");

    // Act
    let unauthorized = cassie.execute_sql(
        &reader,
        "COMMENT ON TABLE issue_845_auth_subject IS 'reader cannot write'",
        vec![],
    );
    let unsupported = [
        "COMMENT ON VIEW issue_845_auth_view IS 'unsupported view comment'",
        "COMMENT ON MATERIALIZED VIEW issue_845_auth_materialized IS 'unsupported materialized view comment'",
        "COMMENT ON TABLE issue_845_auth_view IS 'unsupported view relation'",
        "COMMENT ON TYPE integer IS 'unsupported type comment'",
        "COMMENT ON DATABASE postgres IS 'unsupported database comment'",
        "COMMENT ON INDEX issue_845_missing_index IS 'unsupported index comment'",
        "COMMENT ON FUNCTION issue_845_missing_function() IS 'unsupported routine comment'",
    ]
    .map(|sql| cassie.execute_sql(&root, sql, vec![]));

    // Assert
    assert!(matches!(
        unauthorized,
        Err(CassieError::InsufficientPrivilege)
    ));
    assert!(unsupported
        .iter()
        .all(|result| matches!(result, Err(CassieError::Unsupported(_)))));
}

struct DataDirectory(PathBuf);

impl Drop for DataDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn comment_rows(comment: &str) -> Vec<Vec<Value>> {
    vec![vec![Value::String(comment.to_owned())]]
}

fn data_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("cassie-{label}-{}", uuid::Uuid::new_v4()))
}
