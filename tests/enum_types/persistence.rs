//! Enum types, labels, rows and member identities survive restart on local storage.
//! Authored against the v0.4.0 contract.

use crate::support_composite_types::LocalStore;
use crate::support_enum_types::cell_text;
use cassie::app::Cassie;

fn run(cassie: &Cassie, sql: &str) -> Vec<Vec<cassie::types::Value>> {
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, sql, vec![])
        .unwrap_or_else(|error| panic!("statement failed: {sql}: {error}"))
        .rows()
        .to_vec()
}

fn column(cassie: &Cassie, sql: &str) -> Vec<Option<String>> {
    run(cassie, sql)
        .iter()
        .map(|row| cell_text(&row[0]))
        .collect()
}

fn one(cassie: &Cassie, sql: &str) -> Option<String> {
    let mut cells = column(cassie, sql);
    assert_eq!(cells.len(), 1, "expected one row for {sql}");
    cells.remove(0)
}

fn texts(cells: &[&str]) -> Vec<Option<String>> {
    cells.iter().map(|cell| Some((*cell).to_owned())).collect()
}

const MEMBERS: &str = "SELECT enumlabel || '=' || oid::text || '@' || enumsortorder::text \
     FROM pg_catalog.pg_enum ORDER BY enumsortorder";

/// Creates the schema, type and rows, then applies the alterations the contract
/// requires to be durable.
fn seed(store: &LocalStore) {
    let cassie = store.open();
    run(&cassie, "CREATE SCHEMA app");
    run(
        &cassie,
        "CREATE TYPE app.state_code AS ENUM ('waiting', 'ready', 'done')",
    );
    run(
        &cassie,
        "CREATE TABLE app.jobs (id int PRIMARY KEY, st app.state_code, history app.state_code[])",
    );
    run(
        &cassie,
        "INSERT INTO app.jobs VALUES (1, 'waiting', '{waiting,ready}'), (2, 'done', NULL), (3, NULL, '{}')",
    );
}

fn alter(store: &LocalStore) {
    let cassie = store.open();
    run(
        &cassie,
        "ALTER TYPE app.state_code ADD VALUE 'paused' BEFORE 'ready'",
    );
    run(
        &cassie,
        "ALTER TYPE app.state_code RENAME VALUE 'done' TO 'complete'",
    );
    run(&cassie, "ALTER TYPE app.state_code RENAME TO task_state");
    run(
        &cassie,
        "INSERT INTO app.jobs VALUES (4, 'paused', '{paused}')",
    );
}

#[test]
fn should_keep_enum_rows_across_restart() {
    // Arrange
    let store = LocalStore::new("enum_restart_rows");
    seed(&store);

    // Act
    let cassie = store.open();
    let rows = column(
        &cassie,
        "SELECT coalesce(st::text, 'NULL') FROM app.jobs ORDER BY id",
    );
    let arrays = column(
        &cassie,
        "SELECT coalesce(history::text, 'NULL') FROM app.jobs ORDER BY id",
    );

    // Assert
    assert_eq!(rows, texts(&["waiting", "done", "NULL"]));
    assert_eq!(arrays, texts(&["{waiting,ready}", "NULL", "{}"]));
}

#[test]
fn should_keep_enum_identities_across_restart() {
    // Arrange
    let store = LocalStore::new("enum_restart_oids");
    seed(&store);
    let before_ids = {
        let cassie = store.open();
        let ids = column(
            &cassie,
            "SELECT oid::text || '/' || typarray::text FROM pg_catalog.pg_type WHERE typname = 'state_code'",
        );
        let members = column(&cassie, MEMBERS);
        (ids, members)
    };

    // Act
    let cassie = store.open();
    let after_ids = column(
        &cassie,
        "SELECT oid::text || '/' || typarray::text FROM pg_catalog.pg_type WHERE typname = 'state_code'",
    );
    let after_members = column(&cassie, MEMBERS);

    // Assert
    assert_eq!(before_ids.0.len(), 1);
    assert_eq!(before_ids.0, after_ids, "type and array OIDs are durable");
    assert_eq!(before_ids.1.len(), 3);
    assert_eq!(
        before_ids.1, after_members,
        "member IDs, labels and sort orders are durable"
    );
}

#[test]
fn should_keep_alterations_across_restart() {
    // Arrange
    let store = LocalStore::new("enum_restart_alter");
    seed(&store);
    alter(&store);

    // Act
    let cassie = store.open();
    let labels = column(&cassie, "SELECT enum_range(NULL::app.task_state)::text");
    let rows = column(
        &cassie,
        "SELECT id::text || '=' || coalesce(st::text, 'NULL') FROM app.jobs ORDER BY id",
    );
    let ordered = column(
        &cassie,
        "SELECT id::text FROM app.jobs WHERE st IS NOT NULL ORDER BY st, id",
    );
    let old_name = cassie.execute_sql(
        &cassie.create_session("tester", None),
        "SELECT 'waiting'::app.state_code::text",
        vec![],
    );
    let old_label = cassie.execute_sql(
        &cassie.create_session("tester", None),
        "SELECT 'done'::app.task_state::text",
        vec![],
    );

    // Assert
    assert_eq!(labels, texts(&["{waiting,paused,ready,complete}"]));
    assert_eq!(
        rows,
        texts(&["1=waiting", "2=complete", "3=NULL", "4=paused"])
    );
    assert_eq!(ordered, texts(&["1", "4", "2"]));
    assert!(old_name.is_err(), "the pre-rename type name is gone");
    assert_eq!(
        old_label.map_err(|error| error.sql_state()).err(),
        Some("22P02"),
        "the pre-rename label is no longer input"
    );
}

#[test]
fn should_keep_member_identity_through_rename_value_across_restart() {
    // Arrange
    let store = LocalStore::new("enum_restart_member_identity");
    seed(&store);
    let done_id = {
        let cassie = store.open();
        one(
            &cassie,
            "SELECT oid::text FROM pg_catalog.pg_enum WHERE enumlabel = 'done'",
        )
    };
    alter(&store);

    // Act
    let cassie = store.open();
    let renamed_id = one(
        &cassie,
        "SELECT oid::text FROM pg_catalog.pg_enum WHERE enumlabel = 'complete'",
    );
    let old_label_rows = column(
        &cassie,
        "SELECT oid::text FROM pg_catalog.pg_enum WHERE enumlabel = 'done'",
    );

    // Assert
    assert!(done_id.is_some());
    assert_eq!(done_id, renamed_id, "RENAME VALUE keeps the member ID");
    assert_eq!(old_label_rows, Vec::<Option<String>>::new());
}

#[test]
fn should_keep_type_oids_through_rename_to_across_restart() {
    // Arrange
    let store = LocalStore::new("enum_restart_rename_to_oid");
    seed(&store);
    let before = {
        let cassie = store.open();
        one(
            &cassie,
            "SELECT oid::text || '/' || typarray::text FROM pg_catalog.pg_type WHERE typname = 'state_code'",
        )
    };
    alter(&store);

    // Act
    let cassie = store.open();
    let after = one(
        &cassie,
        "SELECT oid::text || '/' || typarray::text FROM pg_catalog.pg_type WHERE typname = 'task_state'",
    );

    // Assert
    assert!(before.is_some());
    assert_eq!(before, after, "RENAME TO keeps both OIDs");
}

#[test]
fn should_accept_inserts_with_every_label_after_restart() {
    // Arrange
    let store = LocalStore::new("enum_restart_insert_after");
    seed(&store);
    alter(&store);

    // Act
    let cassie = store.open();
    run(
        &cassie,
        "INSERT INTO app.jobs VALUES (5, 'ready', '{complete,paused}')",
    );
    let stored = column(&cassie, "SELECT history::text FROM app.jobs WHERE id = 5");
    let rows = column(
        &cassie,
        "SELECT st::text AS label FROM app.jobs WHERE st > 'paused' ORDER BY st, id",
    );

    // Assert
    assert_eq!(stored, texts(&["{complete,paused}"]));
    assert_eq!(rows, texts(&["ready", "complete"]));
}

#[test]
fn should_keep_a_scalar_index_probe_correct_through_rename_across_restart() {
    // Arrange
    let store = LocalStore::new("enum_restart_index");
    seed(&store);
    {
        let cassie = store.open();
        run(&cassie, "CREATE INDEX jobs_state ON app.jobs (st)");
    }
    alter(&store);

    // Act
    let cassie = store.open();
    let complete = column(
        &cassie,
        "SELECT id::text FROM app.jobs WHERE st = 'complete' ORDER BY id",
    );
    let paused = column(
        &cassie,
        "SELECT id::text FROM app.jobs WHERE st = 'paused' ORDER BY id",
    );

    // Assert
    assert_eq!(complete, texts(&["2"]));
    assert_eq!(paused, texts(&["4"]));
}

#[test]
fn should_not_restore_a_dropped_enum_after_restart() {
    // Arrange
    let store = LocalStore::new("enum_restart_drop");
    {
        let cassie = store.open();
        run(&cassie, "CREATE TYPE state_code AS ENUM ('open')");
        run(&cassie, "DROP TYPE state_code");
    }

    // Act
    let cassie = store.open();
    let types = column(
        &cassie,
        "SELECT typname FROM pg_catalog.pg_type WHERE typname IN ('state_code', '_state_code')",
    );
    let members = column(&cassie, "SELECT enumlabel FROM pg_catalog.pg_enum");
    let recreated = cassie.execute_sql(
        &cassie.create_session("tester", None),
        "CREATE TYPE state_code AS ENUM ('again')",
        vec![],
    );

    // Assert
    assert_eq!(types, Vec::<Option<String>>::new());
    assert_eq!(members, Vec::<Option<String>>::new());
    assert!(recreated.is_ok(), "the name is free after a durable drop");
}

#[test]
fn should_not_reuse_member_ids_of_a_rolled_back_add_value_after_restart() {
    // Arrange
    let store = LocalStore::new("enum_restart_burned_ids");
    {
        let cassie = store.open();
        run(&cassie, "CREATE TYPE state_code AS ENUM ('open')");
        let session = cassie.create_session("tester", None);
        for sql in [
            "BEGIN",
            "ALTER TYPE state_code ADD VALUE 'burned'",
            "ROLLBACK",
        ] {
            cassie
                .execute_sql(&session, sql, vec![])
                .unwrap_or_else(|error| panic!("{sql}: {error}"));
        }
    }

    // Act
    let cassie = store.open();
    run(&cassie, "ALTER TYPE state_code ADD VALUE 'kept'");
    let ids = column(
        &cassie,
        "SELECT oid::text FROM pg_catalog.pg_enum ORDER BY enumsortorder",
    );

    // Assert
    assert_eq!(ids.len(), 2);
    let open: u64 = ids[0].as_deref().unwrap().parse().unwrap();
    let kept: u64 = ids[1].as_deref().unwrap().parse().unwrap();
    assert!(
        kept > open + 1,
        "the ID allocated inside the rolled-back transaction is never reused"
    );
}
