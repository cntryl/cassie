//! A declared replayable SQL transaction keeps its rows and checkpoint atomic across restart.

use crate::app::{Cassie, CassieSession};
use crate::config::CassieRuntimeConfig;

fn open(path: &std::path::Path) -> Cassie {
    let cassie =
        Cassie::new_local_with_data_dir_and_config(path, CassieRuntimeConfig::default()).unwrap();
    cassie.startup().unwrap();
    cassie
}

fn run(cassie: &Cassie, session: &CassieSession, sql: &str) {
    cassie.execute_sql(session, sql, vec![]).unwrap();
}

fn count(cassie: &Cassie, session: &CassieSession, table: &str) -> usize {
    cassie
        .execute_sql(session, &format!("SELECT * FROM {table}"), vec![])
        .unwrap()
        .rows()
        .len()
}

fn path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "cassie-replayable-restart-{}",
        uuid::Uuid::new_v4()
    ))
}

fn declare_and_write(cassie: &Cassie, session: &CassieSession, rows: usize) {
    run(cassie, session, "BEGIN");
    run(
        cassie,
        session,
        "SET LOCAL cassie.projection_write_mode = 'replayable'",
    );
    for value in 0..rows {
        run(
            cassie,
            session,
            &format!("INSERT INTO restart_data (value) VALUES ({value})"),
        );
    }
    run(
        cassie,
        session,
        "INSERT INTO restart_checkpoint (position) VALUES (1)",
    );
}

fn create_tables(cassie: &Cassie, session: &CassieSession) {
    run(cassie, session, "CREATE TABLE restart_data (value INT)");
    run(
        cassie,
        session,
        "CREATE TABLE restart_checkpoint (position INT)",
    );
}

#[test]
fn should_recover_the_committed_projection_after_a_clean_restart() {
    // Arrange
    let path = path();
    let cassie = open(&path);
    let session = cassie.create_session("projector", None);
    create_tables(&cassie, &session);
    declare_and_write(&cassie, &session, 3);
    run(&cassie, &session, "COMMIT");

    // Act
    cassie.shutdown();
    drop(cassie);
    let recovered = open(&path);
    let session = recovered.create_session("projector", None);

    // Assert
    assert_eq!(count(&recovered, &session, "restart_data"), 3);
    assert_eq!(count(&recovered, &session, "restart_checkpoint"), 1);
}

#[test]
fn should_recover_neither_rows_nor_checkpoint_for_an_uncommitted_declared_transaction() {
    // Arrange
    let path = path();
    let cassie = open(&path);
    let session = cassie.create_session("projector", None);
    create_tables(&cassie, &session);
    declare_and_write(&cassie, &session, 3);

    // Act
    cassie.shutdown();
    drop(cassie);
    let recovered = open(&path);
    let session = recovered.create_session("projector", None);

    // Assert
    assert_eq!(count(&recovered, &session, "restart_data"), 0);
    assert_eq!(count(&recovered, &session, "restart_checkpoint"), 0);
}

#[test]
fn should_recover_the_projection_atomically_without_a_shutdown() {
    // Arrange
    let path = path();
    let cassie = open(&path);
    let session = cassie.create_session("projector", None);
    create_tables(&cassie, &session);
    declare_and_write(&cassie, &session, 3);
    run(&cassie, &session, "COMMIT");

    // Act: the process ends without a shutdown. Buffered WAL may not have
    // reached disk, so the committed transaction may be absent, but never partial.
    drop(session);
    drop(cassie);
    let recovered = open(&path);
    let session = recovered.create_session("projector", None);
    let tables = [
        count(&recovered, &session, "restart_data"),
        count(&recovered, &session, "restart_checkpoint"),
    ];

    // Assert
    assert!(tables == [3, 1] || tables == [0, 0]);
}
