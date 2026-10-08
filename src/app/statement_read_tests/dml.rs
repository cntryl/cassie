//! DML source views end before mutation application and constraint authority.
use super::{execute, Cassie, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("strict DML fixture cleanup");
    }
}
struct Fixture {
    cassie: Arc<Cassie>,
    _directory: Directory,
}
impl Fixture {
    fn new(right: i64) -> Self {
        let directory = Directory(
            std::env::temp_dir().join(format!("cassie-dml-view-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&directory.0).expect("create fixture directory");
        let cassie = Arc::new(Cassie::new_with_data_dir(&directory.0).expect("Cassie"));
        cassie.startup().expect("startup");
        let session = cassie.create_session("setup", None);
        execute(
            &cassie,
            &session,
            "CREATE TABLE source_left (id INT PRIMARY KEY, n INT)",
        );
        execute(
            &cassie,
            &session,
            "CREATE TABLE source_right (id INT PRIMARY KEY, n INT)",
        );
        execute(&cassie, &session, "INSERT INTO source_left VALUES (1, 10)");
        execute(
            &cassie,
            &session,
            &format!("INSERT INTO source_right VALUES (1, {right})"),
        );
        Self {
            cassie,
            _directory: directory,
        }
    }

    fn commit_between_sources(
        &self,
        right: i64,
    ) -> (crate::executor::JoinReadProbe, Arc<AtomicBool>) {
        let cassie = Arc::clone(&self.cassie);
        let writer = cassie.create_session("writer", None);
        let observed = Arc::new(AtomicBool::new(false));
        let committed = Arc::clone(&observed);
        let hook = crate::executor::JoinReadProbe::install(move || {
            execute(&cassie, &writer, "BEGIN");
            execute(&cassie, &writer, "UPDATE source_left SET n = 110");
            execute(
                &cassie,
                &writer,
                &format!("UPDATE source_right SET n = {right}"),
            );
            execute(&cassie, &writer, "COMMIT");
            committed.store(true, Ordering::SeqCst);
        });
        (hook, observed)
    }
}

fn equal_source_exists() -> &'static str {
    "EXISTS (SELECT l.n FROM source_left l CROSS JOIN source_right r WHERE l.n = r.n)"
}

#[test]
fn should_share_insert_select_data_across_joined_sources() {
    // Arrange
    let fixture = Fixture::new(20);
    let reader = fixture.cassie.create_session("reader", None);
    execute(
        &fixture.cassie,
        &reader,
        "CREATE TABLE destination (a INT, b INT)",
    );
    let (hook, committed) = fixture.commit_between_sources(120);

    // Act
    execute(&fixture.cassie, &reader, "INSERT INTO destination (a, b) SELECT l.n, r.n FROM source_left l JOIN source_right r ON l.id = r.id");
    drop(hook);
    let copied = execute(&fixture.cassie, &reader, "SELECT a, b FROM destination");
    let fresh = execute(
        &fixture.cassie,
        &reader,
        "SELECT l.n, r.n FROM source_left l JOIN source_right r ON l.id = r.id",
    );
    drop(fixture);

    // Assert
    assert!(
        committed.load(Ordering::SeqCst),
        "the writer committed between source reads"
    );
    assert_eq!(copied, vec![vec![Value::Int64(10), Value::Int64(20)]]);
    assert_eq!(fresh, vec![vec![Value::Int64(110), Value::Int64(120)]]);
}

#[test]
fn should_share_update_predicate_data_across_exists_sources() {
    // Arrange
    let fixture = Fixture::new(10);
    let reader = fixture.cassie.create_session("reader", None);
    execute(
        &fixture.cassie,
        &reader,
        "CREATE TABLE mutation_target (id INT PRIMARY KEY, n INT)",
    );
    execute(
        &fixture.cassie,
        &reader,
        "INSERT INTO mutation_target VALUES (1, 1)",
    );
    let (hook, committed) = fixture.commit_between_sources(110);

    // Act
    execute(
        &fixture.cassie,
        &reader,
        &format!(
            "UPDATE mutation_target SET n = 99 WHERE {}",
            equal_source_exists()
        ),
    );
    drop(hook);
    let updated = execute(&fixture.cassie, &reader, "SELECT n FROM mutation_target");
    drop(fixture);

    // Assert
    assert!(
        committed.load(Ordering::SeqCst),
        "the writer committed between predicate reads"
    );
    assert_eq!(updated, vec![vec![Value::Int64(99)]]);
}

#[test]
fn should_share_delete_predicate_data_across_exists_sources() {
    // Arrange
    let fixture = Fixture::new(10);
    let reader = fixture.cassie.create_session("reader", None);
    execute(
        &fixture.cassie,
        &reader,
        "CREATE TABLE mutation_target (id INT PRIMARY KEY, n INT)",
    );
    execute(
        &fixture.cassie,
        &reader,
        "INSERT INTO mutation_target VALUES (1, 1)",
    );
    let (hook, committed) = fixture.commit_between_sources(110);

    // Act
    execute(
        &fixture.cassie,
        &reader,
        &format!(
            "DELETE FROM mutation_target WHERE {}",
            equal_source_exists()
        ),
    );
    drop(hook);
    let remaining = execute(&fixture.cassie, &reader, "SELECT n FROM mutation_target");
    drop(fixture);

    // Assert
    assert!(
        committed.load(Ordering::SeqCst),
        "the writer committed between predicate reads"
    );
    assert_eq!(remaining, Vec::<Vec<Value>>::new());
}

#[test]
fn should_validate_insert_select_foreign_keys_after_the_source_scope_ends() {
    // Arrange
    let fixture = Fixture::new(20);
    let reader = fixture.cassie.create_session("reader", None);
    execute(
        &fixture.cassie,
        &reader,
        "CREATE TABLE source_parents (id INT PRIMARY KEY)",
    );
    execute(&fixture.cassie, &reader, "CREATE TABLE source_children (id INT PRIMARY KEY, parent_id INT REFERENCES source_parents(id))");
    execute(
        &fixture.cassie,
        &reader,
        "INSERT INTO source_parents VALUES (1)",
    );
    let writer = fixture.cassie.create_session("writer", None);
    let nested = Arc::clone(&fixture.cassie);
    let committed = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&committed);
    let hook = crate::executor::JoinReadProbe::install(move || {
        execute(&nested, &writer, "DELETE FROM source_parents WHERE id = 1");
        observed.store(true, Ordering::SeqCst);
    });

    // Act
    let result = fixture.cassie.execute_sql(&reader, "INSERT INTO source_children (id, parent_id) SELECT l.id, r.id FROM source_left l JOIN source_right r ON l.id = r.id", vec![]);
    drop(hook);
    let children = execute(
        &fixture.cassie,
        &reader,
        "SELECT parent_id FROM source_children",
    );
    let parents = execute(&fixture.cassie, &reader, "SELECT id FROM source_parents");
    drop(fixture);

    // Assert
    assert!(
        committed.load(Ordering::SeqCst),
        "parent removal completed during source reads"
    );
    let message = result
        .expect_err("fresh mutation authority sees the removed parent")
        .to_string();
    assert!(message.contains("foreign key"), "{message}");
    assert_eq!(children, Vec::<Vec<Value>>::new());
    assert_eq!(parents, Vec::<Vec<Value>>::new());
}

#[test]
fn should_cancel_insert_select_without_retaining_source_owners() {
    // Arrange
    let fixture = Fixture::new(20);
    let reader = fixture.cassie.create_session("reader", None);
    execute(
        &fixture.cassie,
        &reader,
        "CREATE TABLE destination (a INT, b INT)",
    );
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    let cancel = cancellation.clone();
    let mut limits = fixture.cassie.runtime.limits();
    limits.query_memory_budget_bytes = 64 * 1024;
    let controls = crate::runtime::QueryExecutionControls::with_cancellation(
        &limits,
        std::time::Instant::now(),
        cancellation,
    );
    let reached = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&reached);
    let hook = crate::executor::JoinReadProbe::install(move || {
        observed.store(true, Ordering::SeqCst);
        cancel.cancel();
    });

    // Act
    let result = fixture.cassie.execute_sql_with_controls(&reader,
        "INSERT INTO destination (a, b) SELECT l.n, r.n FROM source_left l JOIN source_right r ON l.id = r.id",
        vec![], crate::app::ExecutionMode::SimpleQuery, &controls,
    );
    drop(hook);
    let retained = controls.current_query_memory_bytes();
    let workers = fixture
        .cassie
        .runtime
        .snapshot()
        .runtime
        .active_operator_workers;
    let copied = execute(&fixture.cassie, &reader, "SELECT a, b FROM destination");
    drop(fixture);

    // Assert
    assert!(
        reached.load(Ordering::SeqCst),
        "cancellation followed the first source read"
    );
    assert!(
        matches!(result, Err(crate::app::CassieError::QueryCancelled)),
        "{result:?}"
    );
    assert_eq!(copied, Vec::<Vec<Value>>::new());
    assert_eq!(retained, 0);
    assert_eq!(workers, 0);
}
