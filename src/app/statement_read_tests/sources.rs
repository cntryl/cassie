//! Qualification of committed source versions, including empty acquisition.
use super::{execute, CaptureHook, Cassie, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct Directory(PathBuf);
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).expect("strict source fixture cleanup");
    }
}
struct Fixture {
    cassie: Arc<Cassie>,
    _directory: Directory,
}
impl Fixture {
    fn new() -> Self {
        let directory = Directory(
            std::env::temp_dir().join(format!("cassie-source-view-{}", uuid::Uuid::new_v4())),
        );
        std::fs::create_dir_all(&directory.0).expect("create fixture directory");
        let cassie = Arc::new(Cassie::new_with_data_dir(&directory.0).expect("Cassie"));
        cassie.startup().expect("startup");
        let setup = cassie.create_session("setup", None);
        for table in ["read_left", "read_right"] {
            execute(
                &cassie,
                &setup,
                &format!("CREATE TABLE {table} (id INT PRIMARY KEY, n INT)"),
            );
            execute(
                &cassie,
                &setup,
                &format!("INSERT INTO {table} VALUES (1, 10), (2, 20)"),
            );
        }
        Self {
            cassie,
            _directory: directory,
        }
    }

    fn commit_between_sources(&self) -> (crate::executor::JoinReadProbe, Arc<AtomicBool>) {
        let cassie = Arc::clone(&self.cassie);
        let writer = cassie.create_session("writer", None);
        let observed = Arc::new(AtomicBool::new(false));
        let committed = Arc::clone(&observed);
        let hook = crate::executor::JoinReadProbe::install(move || {
            execute(&cassie, &writer, "BEGIN");
            execute(&cassie, &writer, "UPDATE read_left SET n = n + 100");
            execute(&cassie, &writer, "UPDATE read_right SET n = n + 100");
            execute(&cassie, &writer, "COMMIT");
            committed.store(true, Ordering::SeqCst);
        });
        (hook, observed)
    }
}

fn original_pairs() -> Vec<Vec<Value>> {
    vec![
        vec![Value::Int64(10), Value::Int64(10)],
        vec![Value::Int64(20), Value::Int64(20)],
    ]
}
fn fresh_pairs() -> Vec<Vec<Value>> {
    vec![
        vec![Value::Int64(110), Value::Int64(110)],
        vec![Value::Int64(120), Value::Int64(120)],
    ]
}

#[test]
fn should_keep_multirow_join_sources_on_the_captured_version() {
    // Arrange
    let fixture = Fixture::new();
    let reader = fixture.cassie.create_session("reader", None);
    let sql = "SELECT l.n, r.n FROM read_left l JOIN read_right r ON l.id = r.id ORDER BY l.id";
    let (hook, committed) = fixture.commit_between_sources();

    // Act
    let captured = execute(&fixture.cassie, &reader, sql);
    drop(hook);
    let fresh = execute(&fixture.cassie, &reader, sql);
    drop(fixture);

    // Assert
    assert!(
        committed.load(Ordering::SeqCst),
        "commit occurred between reads"
    );
    assert_eq!(captured, original_pairs());
    assert_eq!(fresh, fresh_pairs());
}

#[test]
fn should_keep_repeated_cte_sources_on_the_captured_version() {
    // Arrange
    let fixture = Fixture::new();
    let reader = fixture.cassie.create_session("reader", None);
    let sql = "WITH repeated AS (SELECT id, n FROM read_left) SELECT l.n, r.n FROM repeated l JOIN repeated r ON l.id = r.id ORDER BY l.id";
    let (hook, committed) = fixture.commit_between_sources();

    // Act
    let captured = execute(&fixture.cassie, &reader, sql);
    drop(hook);
    let fresh = execute(&fixture.cassie, &reader, sql);
    drop(fixture);

    // Assert
    assert!(
        committed.load(Ordering::SeqCst),
        "commit occurred between CTE consumers"
    );
    assert_eq!(captured, original_pairs());
    assert_eq!(fresh, fresh_pairs());
}

#[test]
fn should_keep_derived_subquery_sources_on_the_captured_version() {
    // Arrange
    let fixture = Fixture::new();
    let reader = fixture.cassie.create_session("reader", None);
    let sql = "SELECT l.n, r.n FROM (SELECT id, n FROM read_left) l JOIN (SELECT id, n FROM read_right) r ON l.id = r.id ORDER BY l.id";
    let (hook, committed) = fixture.commit_between_sources();

    // Act
    let captured = execute(&fixture.cassie, &reader, sql);
    drop(hook);
    let fresh = execute(&fixture.cassie, &reader, sql);
    drop(fixture);

    // Assert
    assert!(
        committed.load(Ordering::SeqCst),
        "commit occurred between derived sources"
    );
    assert_eq!(captured, original_pairs());
    assert_eq!(fresh, fresh_pairs());
}

#[test]
fn should_capture_empty_sources_before_later_committed_inserts() {
    // Arrange
    let fixture = Fixture::new();
    let reader = fixture.cassie.create_session("reader", None);
    execute(&fixture.cassie, &reader, "DELETE FROM read_left");
    let cassie = Arc::clone(&fixture.cassie);
    let writer = cassie.create_session("writer", None);
    let committed = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&committed);
    let hook = CaptureHook::install(move || {
        execute(&cassie, &writer, "INSERT INTO read_left VALUES (1, 110)");
        observed.store(true, Ordering::SeqCst);
    });
    let sql = "SELECT l.n, r.n FROM read_left l JOIN read_right r ON l.id = r.id";

    // Act
    let captured = execute(&fixture.cassie, &reader, sql);
    drop(hook);
    let fresh = execute(&fixture.cassie, &reader, sql);
    drop(fixture);

    // Assert
    assert!(
        committed.load(Ordering::SeqCst),
        "insert committed after capture"
    );
    assert_eq!(captured, Vec::<Vec<Value>>::new());
    assert_eq!(fresh, vec![vec![Value::Int64(110), Value::Int64(10)]]);
}

#[test]
fn should_keep_visible_right_rows_captured_after_an_empty_left_read() {
    // Arrange
    let fixture = Fixture::new();
    let reader = fixture.cassie.create_session("reader", None);
    execute(&fixture.cassie, &reader, "DELETE FROM read_left");
    let cassie = Arc::clone(&fixture.cassie);
    let writer = cassie.create_session("writer", None);
    let committed = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&committed);
    let hook = crate::executor::JoinReadProbe::install(move || {
        execute(&cassie, &writer, "BEGIN");
        execute(&cassie, &writer, "INSERT INTO read_left VALUES (1, 110)");
        execute(&cassie, &writer, "UPDATE read_right SET n = n + 100");
        execute(&cassie, &writer, "COMMIT");
        observed.store(true, Ordering::SeqCst);
    });
    let sql =
        "SELECT l.n, r.n FROM read_left l RIGHT JOIN read_right r ON l.id = r.id ORDER BY r.id";

    // Act
    let captured = execute(&fixture.cassie, &reader, sql);
    drop(hook);
    let fresh = execute(&fixture.cassie, &reader, sql);
    drop(fixture);

    // Assert
    assert!(
        committed.load(Ordering::SeqCst),
        "commit followed the empty left read"
    );
    assert_eq!(
        captured,
        vec![
            vec![Value::Null, Value::Int64(10)],
            vec![Value::Null, Value::Int64(20)]
        ]
    );
    assert_eq!(
        fresh,
        vec![
            vec![Value::Int64(110), Value::Int64(110)],
            vec![Value::Null, Value::Int64(120)]
        ]
    );
}
