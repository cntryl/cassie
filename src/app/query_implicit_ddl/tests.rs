//! Standalone DDL publication and no-op catalog ownership laws.

use super::*;
use crate::config::{CassieRuntimeConfig, CassieRuntimeLimits};
use crate::runtime::QueryCancellationHandle;
use std::sync::{mpsc, Arc, Barrier};
use std::time::Instant;

const CREATE: &str = "CREATE TABLE IF NOT EXISTS startup_checkpoint \
    (identity TEXT PRIMARY KEY, cursor TEXT NOT NULL, revision BIGINT NOT NULL)";

fn fixture() -> Cassie {
    let cassie =
        Cassie::new_in_memory_with_config(CassieRuntimeConfig::default()).expect("memory fixture");
    cassie.startup().expect("startup");
    cassie
}

#[test]
fn should_publish_implicit_table_creation_before_returning_its_command() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    session
        .begin_implicit_transaction()
        .expect("implicit owner");

    // Act
    let result = cassie
        .execute_sql(&session, CREATE, vec![])
        .expect("create");
    let observer = cassie.create_session("observer", None);
    let visible = cassie.execute_sql(&observer, "SELECT * FROM startup_checkpoint", vec![]);

    // Assert
    assert_eq!(result.command(), "CREATE TABLE");
    assert_eq!(session.transaction_status(), "idle");
    assert!(visible
        .expect("committed table visible before command")
        .rows()
        .is_empty());
}

#[test]
fn should_serialize_concurrent_idempotent_table_creation() {
    // Arrange
    let cassie = fixture();
    let start = Arc::new(Barrier::new(12));

    // Act
    let results = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..12)
            .map(|_| {
                let start = Arc::clone(&start);
                let cassie = &cassie;
                scope.spawn(move || {
                    let session = cassie.create_session("tester", None);
                    session
                        .begin_implicit_transaction()
                        .expect("implicit owner");
                    start.wait();
                    let result = cassie.execute_sql(&session, CREATE, vec![]);
                    if result.is_ok() && session.is_implicit_transaction() {
                        cassie.commit_transaction(&session)?;
                    }
                    result.map(|result| (result.command().to_owned(), session.transaction_status()))
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().expect("DDL worker"))
            .collect::<Vec<_>>()
    });

    // Assert
    for result in results {
        let (command, status) = result.expect("every startup session succeeds");
        assert_eq!(command, "CREATE TABLE");
        assert_eq!(status, "idle");
    }
    let schema = cassie
        .catalog
        .get_schema("startup_checkpoint")
        .expect("one table");
    assert_eq!(schema.fields.len(), 3);
}

#[test]
fn should_preserve_schema_epoch_after_an_implicit_noop() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, CREATE, vec![])
        .expect("existing table");
    let epoch = cassie.midge.schema_epoch().expect("schema epoch");
    let version = cassie.catalog.version();
    session
        .begin_implicit_transaction()
        .expect("implicit owner");

    // Act
    cassie
        .execute_sql(&session, CREATE, vec![])
        .expect("no-op create");
    if session.is_implicit_transaction() {
        cassie.commit_transaction(&session).expect("finish no-op");
    }

    // Assert
    assert_eq!(cassie.catalog.version(), version);
    assert_eq!(cassie.midge.schema_epoch().expect("unchanged epoch"), epoch);
    assert_eq!(session.transaction_status(), "idle");
}

#[test]
fn should_commit_unchanged_ddl_without_replacing_a_newer_catalog() {
    // Arrange
    let cassie = fixture();
    let owner = cassie.create_session("owner", None);
    let concurrent = cassie.create_session("concurrent", None);
    cassie
        .execute_sql(&owner, CREATE, vec![])
        .expect("existing table");
    cassie
        .execute_sql(&owner, "BEGIN", vec![])
        .expect("explicit owner");
    cassie
        .execute_sql(&owner, CREATE, vec![])
        .expect("unchanged catalog snapshot");
    cassie
        .execute_sql(&concurrent, "CREATE TABLE unrelated (n INT)", vec![])
        .expect("unrelated publication");
    let version = cassie.catalog.version();
    let epoch = cassie.midge.schema_epoch().expect("latest epoch");

    // Act
    let result = cassie.execute_sql(&owner, "COMMIT", vec![]);

    // Assert
    assert_eq!(result.expect("no-op cannot conflict").command(), "COMMIT");
    assert!(cassie.catalog.get_schema("unrelated").is_some());
    assert_eq!(cassie.catalog.version(), version);
    assert_eq!(cassie.midge.schema_epoch().expect("unchanged epoch"), epoch);
}

#[test]
fn should_preserve_explicit_transactional_ddl_conflicts() {
    // Arrange
    let cassie = fixture();
    let owner = cassie.create_session("owner", None);
    let concurrent = cassie.create_session("concurrent", None);
    cassie
        .execute_sql(&owner, "BEGIN", vec![])
        .expect("explicit owner");
    cassie
        .execute_sql(&owner, "CREATE TABLE private_ddl (n INT)", vec![])
        .expect("real staged DDL");
    cassie
        .execute_sql(&concurrent, "CREATE TABLE public_ddl (n INT)", vec![])
        .expect("concurrent publication");

    // Act
    let result = cassie.execute_sql(&owner, "COMMIT", vec![]);

    // Assert
    assert_eq!(
        result
            .expect_err("real snapshot conflict")
            .descriptor()
            .sql_state,
        "40001"
    );
    assert!(cassie.catalog.get_schema("private_ddl").is_none());
    assert!(cassie.catalog.get_schema("public_ddl").is_some());
    owner.rollback_transaction();
}

#[test]
fn should_publish_staged_rows_after_an_unchanged_catalog_statement() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, CREATE, vec![])
        .expect("existing table");
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("explicit owner");
    cassie
        .execute_sql(&session, CREATE, vec![])
        .expect("unchanged DDL");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO startup_checkpoint VALUES ('p', 'e', 1)",
            vec![],
        )
        .expect("staged row");

    // Act
    cassie
        .execute_sql(&session, "COMMIT", vec![])
        .expect("commit staged row");
    let observer = cassie.create_session("observer", None);
    let rows = cassie
        .execute_sql(&observer, "SELECT revision FROM startup_checkpoint", vec![])
        .expect("committed row");

    // Assert
    assert_eq!(rows.rows(), &[vec![Value::Int64(1)]]);
}

#[test]
fn should_release_transferred_catalog_publication_before_the_next_query() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(&session, CREATE, vec![])
        .expect("existing table");
    cassie
        .execute_sql(&session, "BEGIN", vec![])
        .expect("explicit owner");
    cassie
        .execute_sql(
            &session,
            "INSERT INTO startup_checkpoint VALUES ('p', 'e', 1)",
            vec![],
        )
        .expect("staged row with post-commit maintenance");
    let publication = cassie.catalog_publication.write();

    // Act
    cassie
        .commit_transaction_holding_catalog(&session, publication)
        .expect("commit with transferred guard");
    let available = cassie.catalog_publication.try_write().is_some();
    let result = cassie.execute_sql(&session, "SELECT revision FROM startup_checkpoint", vec![]);

    // Assert
    assert!(available, "commit retires its owned publication guard");
    assert_eq!(
        result.expect("next query can acquire publication").rows(),
        &[vec![Value::Int64(1)]]
    );
}

#[test]
fn should_cancel_an_implicit_ddl_publication_wait_before_staging() {
    // Arrange
    let cassie = fixture();
    let cancellation = QueryCancellationHandle::new();
    let controls = QueryExecutionControls::with_cancellation(
        &cassie.runtime.limits(),
        Instant::now(),
        cancellation.clone(),
    );
    let publication = cassie.catalog_publication.read();
    let (entered, waiting) = mpsc::sync_channel(1);

    // Act
    let result = std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let mut entered = Some(entered);
            lock_publication(&cassie, &controls, || {
                if let Some(entered) = entered.take() {
                    entered.send(()).expect("wait observer");
                }
            })
            .map(drop)
        });
        waiting
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("at publication wait");
        cancellation.cancel();
        worker.join().expect("wait worker")
    });
    drop(publication);

    // Assert
    assert_eq!(
        result
            .expect_err("cancelled lock wait")
            .descriptor()
            .sql_state,
        "57014"
    );
    assert!(cassie.catalog.get_schema("startup_checkpoint").is_none());
    assert_eq!(controls.current_query_memory_bytes(), 0);
}

#[test]
fn should_admit_implicit_table_output_before_mutation() {
    // Arrange
    let cassie = fixture();
    let session = cassie.create_session("tester", None);
    session
        .begin_implicit_transaction()
        .expect("implicit owner");
    let controls = QueryExecutionControls::from_limits(
        &CassieRuntimeLimits {
            query_memory_budget_bytes: 0,
            ..CassieRuntimeLimits::default()
        },
        Instant::now(),
    );
    let epoch = cassie.midge.schema_epoch().expect("schema epoch");

    // Act
    let result = cassie.execute_sql_with_controls(
        &session,
        CREATE,
        vec![],
        ExecutionMode::SimpleQuery,
        &controls,
    );
    session.rollback_transaction();

    // Assert
    assert_eq!(
        result.expect_err("denied output").descriptor().sql_state,
        "54000"
    );
    assert!(cassie.catalog.get_schema("startup_checkpoint").is_none());
    assert_eq!(cassie.midge.schema_epoch().expect("unchanged epoch"), epoch);
    assert_eq!(controls.current_query_memory_bytes(), 0);
}
