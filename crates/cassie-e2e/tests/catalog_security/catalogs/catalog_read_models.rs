//! Live catalog discovery uses real database-scoped metadata and survives lifecycle changes.
use crate::support_sql_fixture::sql_fixture;
use cassie::app::Cassie;
use cassie::types::Value;

#[test]
fn should_report_index_presence_given_index_creation_when_pg_tables_is_queried() {
    // Arrange
    let fixture = sql_fixture(
        "catalog209-index-presence",
        &["CREATE TABLE probe(id INTEGER)"],
    );
    let query = "SELECT hasindexes FROM pg_catalog.pg_tables WHERE schemaname='public' AND tablename='probe'";
    // Act
    let plain = fixture.execute(query);
    fixture
        .execute("CREATE INDEX probe_idx ON probe(id)")
        .expect("create index");
    let indexed = fixture.execute(query);
    // Assert
    assert_eq!(
        plain.expect("plain table").rows(),
        &[vec![Value::Bool(false)]]
    );
    assert_eq!(
        indexed.expect("indexed table").rows(),
        &[vec![Value::Bool(true)]]
    );
}

#[test]
fn should_reflect_relation_lifecycle_given_live_metadata_when_pg_tables_is_queried() {
    // Arrange
    let fixture = sql_fixture(
        "catalog209-table-lifecycle",
        &["CREATE TABLE before_name(id INTEGER)"],
    );
    // Act
    fixture
        .execute("ALTER TABLE before_name RENAME TO after_name")
        .expect("rename");
    let renamed = fixture.execute("SELECT tablename FROM pg_catalog.pg_tables WHERE tablename IN ('before_name','after_name') ORDER BY tablename");
    fixture.execute("DROP TABLE after_name").expect("drop");
    let dropped = fixture.execute("SELECT tablename FROM pg_catalog.pg_tables WHERE tablename IN ('before_name','after_name') ORDER BY tablename");
    // Assert
    assert_eq!(
        renamed.expect("renamed table").rows(),
        &[vec![Value::String("after_name".into())]]
    );
    assert_eq!(dropped.expect("dropped table").rows(), &[] as &[Vec<Value>]);
}

#[test]
fn should_remove_dropped_views_given_live_metadata_when_pg_views_is_queried() {
    // Arrange
    let fixture = sql_fixture(
        "catalog209-view-lifecycle",
        &["CREATE VIEW probe_view AS SELECT 42 AS answer"],
    );
    // Act
    let before =
        fixture.execute("SELECT viewname FROM pg_catalog.pg_views WHERE viewname='probe_view'");
    fixture.execute("DROP VIEW probe_view").expect("drop view");
    let after =
        fixture.execute("SELECT viewname FROM pg_catalog.pg_views WHERE viewname='probe_view'");
    // Assert
    assert_eq!(
        before.expect("view").rows(),
        &[vec![Value::String("probe_view".into())]]
    );
    assert_eq!(after.expect("dropped view").rows(), &[] as &[Vec<Value>]);
}

#[test]
fn should_remove_dropped_sequences_given_live_metadata_when_pg_sequences_is_queried() {
    // Arrange
    let fixture = sql_fixture(
        "catalog209-sequence-lifecycle",
        &["CREATE SEQUENCE probe_sequence"],
    );
    // Act
    let before = fixture.execute(
        "SELECT sequencename FROM pg_catalog.pg_sequences WHERE sequencename='probe_sequence'",
    );
    fixture
        .execute("DROP SEQUENCE probe_sequence")
        .expect("drop sequence");
    let after = fixture.execute(
        "SELECT sequencename FROM pg_catalog.pg_sequences WHERE sequencename='probe_sequence'",
    );
    // Assert
    assert_eq!(
        before.expect("sequence").rows(),
        &[vec![Value::String("probe_sequence".into())]]
    );
    assert_eq!(
        after.expect("dropped sequence").rows(),
        &[] as &[Vec<Value>]
    );
}

#[test]
fn should_scope_relation_discovery_given_same_names_in_two_databases_when_catalogs_are_queried() {
    // Arrange
    let fixture = sql_fixture(
        "catalog209-database-isolation",
        &[
            "CREATE DATABASE other_catalog_db",
            "CREATE TABLE local_table(id INTEGER)",
            "CREATE VIEW local_view AS SELECT 1 AS id",
            "CREATE SEQUENCE local_sequence",
        ],
    );
    let other = fixture
        .cassie
        .create_session("tester", Some("other_catalog_db".into()));
    for sql in [
        "CREATE TABLE remote_table(id INTEGER)",
        "CREATE VIEW remote_view AS SELECT 2 AS id",
        "CREATE SEQUENCE remote_sequence",
    ] {
        fixture
            .cassie
            .execute_sql(&other, sql, vec![])
            .expect("other database object");
    }
    // Act
    let cases = [
        ("pg_tables", "tablename", "local_table", "remote_table"),
        ("pg_views", "viewname", "local_view", "remote_view"),
        (
            "pg_sequences",
            "sequencename",
            "local_sequence",
            "remote_sequence",
        ),
    ];
    let results = cases.map(|(view,column,local,remote)| {
        let query = format!("SELECT {column} FROM pg_catalog.{view} WHERE {column} IN ('{local}','{remote}') ORDER BY {column}");
        (fixture.execute(&query), fixture.cassie.execute_sql(&other, &query, vec![]), local, remote)
    });
    // Assert
    for (local, remote, local_name, remote_name) in results {
        assert_eq!(
            local.expect("current database").rows(),
            &[vec![Value::String(local_name.into())]]
        );
        assert_eq!(
            remote.expect("other database").rows(),
            &[vec![Value::String(remote_name.into())]]
        );
    }
}

#[test]
fn should_restore_relation_discovery_given_persisted_objects_when_the_catalog_restarts() {
    // Arrange
    crate::support_sql::use_local_storage();
    let directory = crate::support_catalog_directory::CatalogDirectory::new("read-model-restart");
    {
        let cassie = Cassie::new_with_data_dir(&directory.0).expect("first instance");
        let session = cassie.create_session("tester", None);
        for sql in [
            "CREATE TABLE restored_table(id INTEGER)",
            "CREATE VIEW restored_view AS SELECT 1 AS id",
            "CREATE SEQUENCE restored_sequence",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .expect("persist object");
        }
    }
    // Act
    let reopened = Cassie::new_with_data_dir(&directory.0).expect("reopen");
    reopened.startup().expect("startup");
    let session = reopened.create_session("tester", None);
    let results = [
        ("pg_tables", "tablename", "restored_table"),
        ("pg_views", "viewname", "restored_view"),
        ("pg_sequences", "sequencename", "restored_sequence"),
    ]
    .map(|(view, column, name)| {
        let query = format!("SELECT {column} FROM pg_catalog.{view} WHERE {column}='{name}'");
        (reopened.execute_sql(&session, &query, vec![]), name)
    });
    // Assert
    for (result, name) in results {
        assert_eq!(
            result.expect("restored catalog row").rows(),
            &[vec![Value::String(name.into())]]
        );
    }
}

#[test]
fn should_identify_serial_as_non_identity_given_ordinary_and_serial_columns_when_column_metadata_is_read(
) {
    // Arrange
    let fixture = sql_fixture(
        "catalog209-column-generation",
        &["CREATE TABLE identity_probe(id SERIAL, note TEXT)"],
    );
    // Act
    let result = fixture.execute("SELECT column_name,is_identity,identity_generation,is_generated FROM information_schema.columns WHERE table_name='identity_probe' ORDER BY ordinal_position");
    // Assert
    assert_eq!(
        result.expect("ordinary column identity").rows(),
        &[
            vec![
                Value::String("id".into()),
                Value::String("NO".into()),
                Value::Null,
                Value::String("NEVER".into())
            ],
            vec![
                Value::String("note".into()),
                Value::String("NO".into()),
                Value::Null,
                Value::String("NEVER".into())
            ],
        ]
    );
}

#[test]
fn should_report_sequence_state_given_successive_allocations_when_pg_sequences_is_queried() {
    // Arrange
    let fixture = sql_fixture(
        "catalog209-sequence-state",
        &["CREATE TABLE state_probe(id SERIAL, note TEXT)"],
    );
    let query =
        "SELECT last_value FROM pg_catalog.pg_sequences WHERE sequencename='state_probe_id_seq'";
    // Act
    let unused = fixture.execute(query);
    fixture
        .execute("INSERT INTO state_probe(note) VALUES ('first')")
        .expect("first allocation");
    let first = fixture.execute(query);
    fixture
        .execute("INSERT INTO state_probe(note) VALUES ('second')")
        .expect("second allocation");
    let second = fixture.execute(query);
    // Assert
    assert_eq!(
        unused.expect("unused sequence").rows(),
        &[vec![Value::Null]]
    );
    assert_eq!(
        first.expect("allocated sequence").rows(),
        &[vec![Value::Int64(1)]]
    );
    assert_eq!(
        second.expect("reallocated sequence").rows(),
        &[vec![Value::Int64(2)]]
    );
}

#[test]
fn should_report_sequence_bounds_given_supported_serial_widths_when_pg_sequences_is_queried() {
    // Arrange
    let fixture = sql_fixture(
        "catalog209-sequence-width",
        &[
            "CREATE TABLE int_probe(id SERIAL)",
            "CREATE TABLE big_probe(id BIGSERIAL)",
        ],
    );
    // Act
    let result = fixture.execute("SELECT sequencename,data_type,start_value,min_value,max_value,increment_by,cycle,cache_size FROM pg_catalog.pg_sequences WHERE sequencename IN ('int_probe_id_seq','big_probe_id_seq') ORDER BY sequencename");
    // Assert
    assert_eq!(
        result.expect("typed sequence bounds").rows(),
        &[
            vec![
                Value::String("big_probe_id_seq".into()),
                Value::String("bigint".into()),
                Value::Int64(1),
                Value::Int64(1),
                Value::Int64(i64::MAX),
                Value::Int64(1),
                Value::Bool(false),
                Value::Int64(1)
            ],
            vec![
                Value::String("int_probe_id_seq".into()),
                Value::String("integer".into()),
                Value::Int64(1),
                Value::Int64(1),
                Value::Int64(i64::from(i32::MAX)),
                Value::Int64(1),
                Value::Bool(false),
                Value::Int64(1)
            ],
        ]
    );
}

#[test]
fn should_scope_index_presence_given_same_table_names_in_two_databases_when_pg_tables_is_queried() {
    // Arrange
    let fixture = sql_fixture(
        "catalog209-index-database-isolation",
        &[
            "CREATE DATABASE index_other_db",
            "CREATE TABLE shared_table(id INTEGER)",
        ],
    );
    let other = fixture
        .cassie
        .create_session("tester", Some("index_other_db".into()));
    for sql in [
        "CREATE TABLE shared_table(id INTEGER)",
        "CREATE INDEX remote_idx ON shared_table(id)",
    ] {
        fixture
            .cassie
            .execute_sql(&other, sql, vec![])
            .expect("remote indexed table");
    }
    let query = "SELECT hasindexes FROM pg_catalog.pg_tables WHERE schemaname='public' AND tablename='shared_table'";
    // Act
    let local = fixture.execute(query);
    let remote = fixture.cassie.execute_sql(&other, query, vec![]);
    // Assert
    assert_eq!(
        local.expect("local table").rows(),
        &[vec![Value::Bool(false)]]
    );
    assert_eq!(
        remote.expect("remote table").rows(),
        &[vec![Value::Bool(true)]]
    );
}
