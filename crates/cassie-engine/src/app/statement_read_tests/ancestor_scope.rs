//! Ancestor correlation retains the statement's committed and staged view.
use super::{execute, CaptureHook, Cassie, Value};
use crate::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

const SELECT_SQL: &str = "SELECT o.id, EXISTS (SELECT 1 FROM ancestor_outer m WHERE m.id = o.id AND EXISTS (SELECT 1 FROM ancestor_membership i WHERE i.id = o.id)) AS matched FROM ancestor_outer o ORDER BY o.id";

struct Directory(PathBuf);

impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("strict ancestor fixture cleanup");
    }
}

struct Fixture {
    cassie: Arc<Cassie>,
    _directory: Directory,
}

impl Fixture {
    fn new() -> Self {
        let directory = Directory(
            std::env::temp_dir().join(format!("cassie-ancestor-view-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir(&directory.0).expect("fixture directory");
        let mut config = CassieRuntimeConfig::default();
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie =
            Arc::new(Cassie::new_with_data_dir_and_config(&directory.0, config).expect("Cassie"));
        cassie.startup().expect("startup");
        let setup = cassie.create_session("setup", None);
        for sql in [
            "CREATE TABLE ancestor_outer (id BIGINT PRIMARY KEY)",
            "CREATE TABLE ancestor_membership (id BIGINT PRIMARY KEY)",
            "INSERT INTO ancestor_outer VALUES (1), (2)",
            "INSERT INTO ancestor_membership VALUES (1)",
        ] {
            execute(&cassie, &setup, sql);
        }
        Self {
            cassie,
            _directory: directory,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.cassie.shutdown();
    }
}

#[test]
fn should_preserve_ancestor_overlay_after_nested_rollback() {
    // Arrange
    let fixture = Fixture::new();
    let reader = fixture.cassie.create_session("reader", None);
    execute(&fixture.cassie, &reader, "BEGIN");
    execute(
        &fixture.cassie,
        &reader,
        "INSERT INTO ancestor_membership VALUES (2)",
    );
    let cassie = Arc::clone(&fixture.cassie);
    let same_session = reader.clone();
    let rolled_back = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&rolled_back);
    let hook = CaptureHook::install(move || {
        execute(&cassie, &same_session, "ROLLBACK");
        observed.store(true, Ordering::SeqCst);
    });
    // Act
    let captured = execute(&fixture.cassie, &reader, SELECT_SQL);
    drop(hook);
    let fresh = execute(&fixture.cassie, &reader, SELECT_SQL);
    // Assert
    assert!(
        rolled_back.load(Ordering::SeqCst),
        "rollback followed ancestor capture"
    );
    assert_eq!(
        captured,
        vec![
            vec![Value::Int64(1), Value::Bool(true)],
            vec![Value::Int64(2), Value::Bool(true)]
        ]
    );
    assert_eq!(
        fresh,
        vec![
            vec![Value::Int64(1), Value::Bool(true)],
            vec![Value::Int64(2), Value::Bool(false)]
        ]
    );
    assert_eq!(
        fixture.cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
    assert_eq!(
        fixture
            .cassie
            .runtime
            .snapshot()
            .runtime
            .active_operator_workers,
        0
    );
    drop(reader);
    drop(fixture);
}
