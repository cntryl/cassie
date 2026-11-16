//! A declared replayable SQL transaction keeps its rows and checkpoint atomic across restart.

use crate::app::{Cassie, CassieSession};
use crate::config::CassieRuntimeConfig;

const STALE_LOCK: &str = ".midge_leader.lock";
const LEASE: std::time::Duration = std::time::Duration::from_secs(2);

fn open(path: &std::path::Path) -> Cassie {
    let config = CassieRuntimeConfig::default();
    let midge =
        crate::midge::Midge::new_local_short_lease_for_test(path, &config.database, LEASE).unwrap();
    let cassie = Cassie::new_with_midge_and_config(std::sync::Arc::new(midge), config).unwrap();
    cassie.startup().unwrap();
    cassie
}

/// Reopens after a killed process. The dead writer's lease stays held until it
/// expires, so opening is retried for a bounded time; this waits on an external
/// expiry, not on a scheduling race.
fn open_after_kill(path: &std::path::Path) -> Cassie {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let config = CassieRuntimeConfig::default();
        let midge =
            crate::midge::Midge::new_local_short_lease_for_test(path, &config.database, LEASE);
        match midge {
            Ok(midge) => {
                let cassie =
                    Cassie::new_with_midge_and_config(std::sync::Arc::new(midge), config).unwrap();
                cassie.startup().unwrap();
                return cassie;
            }
            Err(error) if std::time::Instant::now() < deadline => {
                let message = error.to_string();
                assert!(message.contains("Writer lease held"));
                if message.contains(STALE_LOCK) {
                    // The kill landed during a lease renewal. Midge will not
                    // remove the lock itself; an operator who has confirmed
                    // the writer is dead removes it by hand.
                    let _ = std::fs::remove_file(path.join(STALE_LOCK));
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            Err(error) => panic!("the dead writer's lease did not expire: {error}"),
        }
    }
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
    declare_mode_and_write(cassie, session, "replayable", rows);
}

fn declare_mode_and_write(cassie: &Cassie, session: &CassieSession, mode: &str, rows: usize) {
    run(cassie, session, "BEGIN");
    run(
        cassie,
        session,
        &format!("SET LOCAL cassie.projection_write_mode = '{mode}'"),
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

const KILL_PATH: &str = "CASSIE_TEST_KILL_CHILD_PATH";
const KILL_MODE: &str = "CASSIE_TEST_KILL_CHILD_MODE";
const KILL_CHILD: &str =
    "app::projection_sql_restart_tests::should_commit_then_abort_as_the_kill_child";

/// Runs this test binary as a child that commits and then aborts without any
/// shutdown, so the store is left exactly as a killed process leaves it.
fn commit_in_a_child_that_aborts(path: &std::path::Path, mode: &str) {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", KILL_CHILD, "--test-threads=1"])
        .env(KILL_PATH, path)
        .env(KILL_MODE, mode)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success());
}

fn recovered_tables(path: &std::path::Path) -> [usize; 2] {
    let recovered = open_after_kill(path);
    let session = recovered.create_session("projector", None);
    [
        count(&recovered, &session, "restart_data"),
        count(&recovered, &session, "restart_checkpoint"),
    ]
}

#[test]
fn should_commit_then_abort_as_the_kill_child() {
    // Arrange
    let (Some(path), Some(mode)) = (std::env::var_os(KILL_PATH), std::env::var_os(KILL_MODE))
    else {
        return;
    };
    let cassie = open(std::path::Path::new(&path));
    let session = cassie.create_session("projector", None);
    create_tables(&cassie, &session);

    // Act
    declare_mode_and_write(&cassie, &session, mode.to_str().unwrap(), 3);
    run(&cassie, &session, "COMMIT");

    // Assert: unreachable. The process ends here without running any teardown.
    std::process::abort();
}

#[test]
fn should_recover_a_strict_declared_commit_after_the_process_is_killed() {
    // Arrange
    let path = path();

    // Act
    commit_in_a_child_that_aborts(&path, "strict");

    // Assert
    assert_eq!(recovered_tables(&path), [3, 1]);
}

#[test]
fn should_recover_a_replayable_commit_all_or_nothing_after_the_process_is_killed() {
    // Arrange
    let path = path();

    // Act
    commit_in_a_child_that_aborts(&path, "replayable");
    let tables = recovered_tables(&path);

    // Assert
    assert!(tables == [3, 1] || tables == [0, 0]);
}
