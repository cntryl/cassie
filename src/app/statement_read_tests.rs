//! Deterministic statement Data visibility barriers; no public runtime hook.

use super::{Cassie, CassieSession};
use crate::types::Value;
use std::cell::RefCell;
use std::sync::Arc;

thread_local! {
    static AFTER_CAPTURE: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    static BEFORE_CAPTURE: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
}

pub(super) fn after_statement_view_capture() {
    let callback = AFTER_CAPTURE.with(|slot| slot.borrow_mut().take());
    if let Some(callback) = callback {
        callback();
    }
}

pub(super) fn before_statement_view_capture() {
    let callback = BEFORE_CAPTURE.with(|slot| slot.borrow_mut().take());
    if let Some(callback) = callback {
        callback();
    }
}

struct CaptureHook;
struct BeforeCaptureHook;

impl BeforeCaptureHook {
    fn install(callback: impl FnOnce() + 'static) -> Self {
        BEFORE_CAPTURE.with(|slot| {
            assert!(slot.borrow().is_none(), "one scoped pre-capture hook");
            *slot.borrow_mut() = Some(Box::new(callback));
        });
        Self
    }
}

impl Drop for BeforeCaptureHook {
    fn drop(&mut self) {
        BEFORE_CAPTURE.with(|slot| slot.borrow_mut().take());
    }
}

impl CaptureHook {
    fn install(callback: impl FnOnce() + 'static) -> Self {
        AFTER_CAPTURE.with(|slot| {
            assert!(slot.borrow().is_none(), "one scoped capture hook");
            *slot.borrow_mut() = Some(Box::new(callback));
        });
        Self
    }
}

impl Drop for CaptureHook {
    fn drop(&mut self) {
        AFTER_CAPTURE.with(|slot| slot.borrow_mut().take());
    }
}

fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) -> Vec<Vec<Value>> {
    cassie.execute_sql(session, sql, vec![]).expect("SQL").rows
}

#[test]
fn should_keep_captured_data_visible_after_another_session_commits() {
    // Arrange
    let path = std::env::temp_dir().join(format!("cassie-statement-data-{}", uuid::Uuid::new_v4()));
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("Cassie"));
    cassie.startup().expect("startup");
    let reader = cassie.create_session("reader", None);
    let writer = cassie.create_session("writer", None);
    execute(
        &cassie,
        &writer,
        "CREATE TABLE statement_rows (id INT PRIMARY KEY, n INT)",
    );
    execute(
        &cassie,
        &writer,
        "INSERT INTO statement_rows VALUES (1, 10), (2, 20)",
    );
    let writer_cassie = Arc::clone(&cassie);
    let _hook = CaptureHook::install(move || {
        execute(
            &writer_cassie,
            &writer,
            "UPDATE statement_rows SET n = n + 100",
        );
    });

    // Act
    let captured = execute(&cassie, &reader, "SELECT n FROM statement_rows ORDER BY id");
    let fresh = execute(&cassie, &reader, "SELECT n FROM statement_rows ORDER BY id");
    drop(cassie);
    std::fs::remove_dir_all(&path).expect("strict fixture cleanup");

    // Assert
    assert_eq!(
        captured,
        vec![vec![Value::Int64(10)], vec![Value::Int64(20)]]
    );
    assert_eq!(
        fresh,
        vec![vec![Value::Int64(110)], vec![Value::Int64(120)]]
    );
}

#[test]
fn should_retain_statement_owner_charge_until_its_cursor_drops() {
    // Arrange
    let path =
        std::env::temp_dir().join(format!("cassie-statement-owner-{}", uuid::Uuid::new_v4()));
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("Cassie"));
    cassie.startup().expect("startup");
    let session = cassie.create_session("reader", None);
    execute(
        &cassie,
        &session,
        "CREATE TABLE owner_rows (id INT PRIMARY KEY, n INT)",
    );
    execute(&cassie, &session, "INSERT INTO owner_rows VALUES (1, 10)");
    let controls = crate::runtime::QueryExecutionControls::from_limits(
        &cassie.runtime.limits(),
        std::time::Instant::now(),
    );
    let owner = crate::midge::adapter::StatementDataRead::capture(
        &cassie.midge,
        &cassie.default_database,
        &controls,
    )
    .expect("capture");
    let admitted = controls.current_query_memory_bytes();
    let scope = crate::midge::adapter::StatementReadScope::enter(Some(&owner));
    let cursor = cassie
        .midge
        .open_row_cursor("owner_rows", crate::midge::adapter::RowDecode::Full)
        .expect("open")
        .expect("cursor");

    // Act
    drop(scope);
    drop(owner);
    let retained = controls.current_query_memory_bytes();
    drop(cursor);
    let released = controls.current_query_memory_bytes();
    drop(cassie);
    std::fs::remove_dir_all(&path).expect("strict fixture cleanup");

    // Assert
    assert!(admitted > 0);
    assert_eq!(
        retained, admitted,
        "live captured cursor retains owner metadata charge"
    );
    assert_eq!(released, 0);
}

#[test]
fn should_share_statement_data_when_result_caching_is_ineligible() {
    // Arrange
    let path = std::env::temp_dir().join(format!(
        "cassie-statement-uncached-{}",
        uuid::Uuid::new_v4()
    ));
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("Cassie"));
    cassie.startup().expect("startup");
    let reader = cassie.create_session("reader", None);
    let writer = cassie.create_session("writer", None);
    execute(
        &cassie,
        &writer,
        "CREATE TABLE uncached_rows (id INT PRIMARY KEY, n INT)",
    );
    execute(&cassie, &writer, "INSERT INTO uncached_rows VALUES (1, 10)");
    let writer_cassie = Arc::clone(&cassie);
    let _hook = CaptureHook::install(move || {
        execute(&writer_cassie, &writer, "UPDATE uncached_rows SET n = 110");
    });

    // Act
    let captured = execute(
        &cassie,
        &reader,
        "SELECT n, current_user() FROM uncached_rows",
    );
    let fresh = execute(
        &cassie,
        &reader,
        "SELECT n, current_user() FROM uncached_rows",
    );
    drop(cassie);
    std::fs::remove_dir_all(&path).expect("strict fixture cleanup");

    // Assert
    assert_eq!(
        captured,
        vec![vec![Value::Int64(10), Value::String("reader".to_string())]]
    );
    assert_eq!(
        fresh,
        vec![vec![Value::Int64(110), Value::String("reader".to_string())]]
    );
}

#[test]
fn should_keep_the_captured_staged_overlay_immutable_during_a_statement() {
    // Arrange
    let path =
        std::env::temp_dir().join(format!("cassie-statement-overlay-{}", uuid::Uuid::new_v4()));
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("Cassie"));
    cassie.startup().expect("startup");
    let session = cassie.create_session("reader", None);
    execute(
        &cassie,
        &session,
        "CREATE TABLE overlay_rows (id INT PRIMARY KEY, n INT)",
    );
    execute(&cassie, &session, "INSERT INTO overlay_rows VALUES (1, 10)");
    execute(&cassie, &session, "BEGIN");
    execute(&cassie, &session, "INSERT INTO overlay_rows VALUES (3, 30)");
    let writer_cassie = Arc::clone(&cassie);
    let same_session = session.clone();
    let _hook = CaptureHook::install(move || {
        execute(
            &writer_cassie,
            &same_session,
            "UPDATE overlay_rows SET n = 130 WHERE id = 3",
        );
        execute(
            &writer_cassie,
            &same_session,
            "INSERT INTO overlay_rows VALUES (4, 40)",
        );
        execute(
            &writer_cassie,
            &same_session,
            "DELETE FROM overlay_rows WHERE id = 1",
        );
    });

    // Act
    let captured = execute(&cassie, &session, "SELECT n FROM overlay_rows ORDER BY id");
    let fresh = execute(&cassie, &session, "SELECT n FROM overlay_rows ORDER BY id");
    execute(&cassie, &session, "ROLLBACK");
    let rolled_back = execute(&cassie, &session, "SELECT n FROM overlay_rows ORDER BY id");
    drop(cassie);
    std::fs::remove_dir_all(&path).expect("strict fixture cleanup");

    // Assert
    assert_eq!(
        captured,
        vec![vec![Value::Int64(10)], vec![Value::Int64(30)]]
    );
    assert_eq!(fresh, vec![vec![Value::Int64(130)], vec![Value::Int64(40)]]);
    assert_eq!(rolled_back, vec![vec![Value::Int64(10)]]);
}

#[test]
fn should_check_capture_controls_before_overlay_admission() {
    // Arrange
    let path = std::env::temp_dir().join(format!("cassie-statement-deny-{}", uuid::Uuid::new_v4()));
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("Cassie"));
    let session = cassie.create_session("reader", None);
    let limits = crate::config::CassieRuntimeLimits {
        query_memory_budget_bytes: 0,
        ..Default::default()
    };
    let cancellation = crate::runtime::QueryCancellationHandle::new();
    cancellation.cancel();
    let cancelled = crate::runtime::QueryExecutionControls::with_cancellation(
        &limits,
        std::time::Instant::now(),
        cancellation,
    );
    let denied =
        crate::runtime::QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    let mut expired =
        crate::runtime::QueryExecutionControls::from_limits(&limits, std::time::Instant::now());
    expired.deadline = Some(std::time::Instant::now() - std::time::Duration::from_secs(1));

    // Act
    let cancelled_result =
        session.capture_statement_read(&cassie.midge, &cassie.default_database, &cancelled);
    let denied_result =
        session.capture_statement_read(&cassie.midge, &cassie.default_database, &denied);
    let expired_result =
        session.capture_statement_read(&cassie.midge, &cassie.default_database, &expired);
    println!("capture controls: cancelled={cancelled_result:?} denied={denied_result:?} expired={expired_result:?}");
    drop(cassie);
    std::fs::remove_dir_all(&path).expect("strict fixture cleanup");

    // Assert
    assert!(matches!(
        cancelled_result,
        Err(super::CassieError::QueryCancelled)
    ));
    assert!(matches!(
        denied_result,
        Err(super::CassieError::ResourceLimit(_))
    ));
    assert!(matches!(
        expired_result,
        Err(super::CassieError::DeadlineExceeded)
    ));
    assert_eq!(cancelled.current_query_memory_bytes(), 0);
    assert_eq!(denied.current_query_memory_bytes(), 0);
    assert_eq!(expired.current_query_memory_bytes(), 0);
}

#[test]
fn should_validate_commit_against_fresh_gated_data_inside_a_read_scope() {
    // Arrange
    let path =
        std::env::temp_dir().join(format!("cassie-statement-commit-{}", uuid::Uuid::new_v4()));
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("Cassie"));
    cassie.startup().expect("startup");
    let reader = cassie.create_session("reader", None);
    let child_writer = cassie.create_session("child_writer", None);
    let deleter = cassie.create_session("deleter", None);
    execute(
        &cassie,
        &reader,
        "CREATE TABLE view_parents (id INT PRIMARY KEY)",
    );
    execute(
        &cassie,
        &reader,
        "CREATE TABLE view_children (parent_id INT REFERENCES view_parents(id))",
    );
    execute(&cassie, &reader, "INSERT INTO view_parents VALUES (1)");
    execute(&cassie, &child_writer, "BEGIN");
    execute(
        &cassie,
        &child_writer,
        "INSERT INTO view_children VALUES (1)",
    );
    let outcome = Arc::new(parking_lot::Mutex::new(None));
    let recorded = Arc::clone(&outcome);
    let writer_cassie = Arc::clone(&cassie);
    let _hook = CaptureHook::install(move || {
        execute(
            &writer_cassie,
            &deleter,
            "DELETE FROM view_parents WHERE id = 1",
        );
        let commit = writer_cassie.execute_sql(&child_writer, "COMMIT", vec![]);
        *recorded.lock() = Some(commit.map(|_| ()).map_err(|error| error.to_string()));
        let _ = writer_cassie.execute_sql(&child_writer, "ROLLBACK", vec![]);
    });

    // Act
    let captured = execute(&cassie, &reader, "SELECT id FROM view_parents");
    let current_parents = execute(&cassie, &reader, "SELECT id FROM view_parents");
    let current_children = execute(&cassie, &reader, "SELECT parent_id FROM view_children");
    let commit = outcome.lock().take().expect("commit attempt");
    println!("gated commit: {commit:?}");
    drop(cassie);
    std::fs::remove_dir_all(&path).expect("strict fixture cleanup");

    // Assert
    assert_eq!(captured, vec![vec![Value::Int64(1)]]);
    assert!(current_parents.is_empty());
    assert!(
        commit.is_err(),
        "commit must reject the deleted committed parent"
    );
    assert!(
        current_children.is_empty(),
        "failed commit publishes no orphan"
    );
}

#[test]
fn should_key_cached_results_to_the_captured_data_epoch() {
    // Arrange
    let path =
        std::env::temp_dir().join(format!("cassie-statement-cache-{}", uuid::Uuid::new_v4()));
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("Cassie"));
    cassie.startup().expect("startup");
    let reader = cassie.create_session("reader", None);
    let writer = cassie.create_session("writer", None);
    execute(
        &cassie,
        &writer,
        "CREATE TABLE epoch_rows (id INT PRIMARY KEY, n INT)",
    );
    execute(&cassie, &writer, "INSERT INTO epoch_rows VALUES (1, 10)");
    let sql = "SELECT n FROM epoch_rows";
    let warm = execute(&cassie, &reader, sql);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (resume_tx, resume_rx) = std::sync::mpsc::channel();
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let old_cassie = Arc::clone(&cassie);
    let old_reader = reader.clone();
    let old_query = std::thread::spawn(move || {
        let _hook = CaptureHook::install(move || {
            ready_tx.send(()).expect("captured old view");
            resume_rx
                .recv_timeout(std::time::Duration::from_secs(30))
                .expect("resume old query");
        });
        let result = execute(&old_cassie, &old_reader, sql);
        finished_tx
            .send(result.clone())
            .expect("old query result stored");
        result
    });
    ready_rx
        .recv_timeout(std::time::Duration::from_secs(30))
        .expect("old query capture");
    let writer_cassie = Arc::clone(&cassie);
    let old_result = Arc::new(parking_lot::Mutex::new(None));
    let recorded = Arc::clone(&old_result);
    let _hook = BeforeCaptureHook::install(move || {
        execute(&writer_cassie, &writer, "UPDATE epoch_rows SET n = 110");
        resume_tx
            .send(())
            .expect("resume old owner after cache invalidation");
        *recorded.lock() = Some(
            finished_rx
                .recv_timeout(std::time::Duration::from_secs(30))
                .expect("old result stored under epochE"),
        );
    });

    // Act
    let newer_capture = execute(&cassie, &reader, sql);
    let old_capture = old_query.join().expect("old captured query");
    let fresh = execute(&cassie, &reader, sql);
    let observed_old = old_result.lock().take().expect("old completion");
    drop(cassie);
    std::fs::remove_dir_all(&path).expect("strict fixture cleanup");

    // Assert
    assert_eq!(warm, vec![vec![Value::Int64(10)]]);
    assert_eq!(old_capture, warm);
    assert_eq!(observed_old, warm);
    assert_eq!(
        newer_capture,
        vec![vec![Value::Int64(110)]],
        "captured new epoch cannot hit old in-flight result"
    );
    assert_eq!(fresh, newer_capture);
}

#[test]
fn should_reuse_the_statement_view_for_raw_database_reads() {
    // Arrange
    let path = std::env::temp_dir().join(format!("cassie-statement-raw-{}", uuid::Uuid::new_v4()));
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("Cassie"));
    cassie.startup().expect("startup");
    let reader = cassie.create_session("reader", None);
    let writer = cassie.create_session("writer", None);
    execute(
        &cassie,
        &writer,
        "CREATE TABLE raw_view_rows (id INT PRIMARY KEY, n INT)",
    );
    execute(&cassie, &writer, "INSERT INTO raw_view_rows VALUES (1, 10)");
    let database = &cassie.default_database;
    let before = cassie
        .midge
        .raw_scan_prefix_database(database, &[])
        .expect("initial Data");
    let controls = crate::runtime::QueryExecutionControls::from_limits(
        &cassie.runtime.limits(),
        std::time::Instant::now(),
    );
    let owner = reader
        .capture_statement_read(&cassie.midge, database, &controls)
        .expect("capture");
    let scope = crate::midge::adapter::StatementReadScope::enter(Some(&owner));
    let overlay_scope = super::SessionReadScope::enter(owner.overlay());

    // Act
    execute(&cassie, &writer, "UPDATE raw_view_rows SET n = 110");
    let captured = cassie
        .midge
        .raw_scan_prefix_database(database, &[])
        .expect("captured Data prefix");
    let page = cassie
        .midge
        .raw_scan_database_page(database, &[], None, usize::MAX)
        .expect("captured page");
    let point_values: Vec<_> = before
        .iter()
        .map(|(key, _)| {
            cassie
                .midge
                .raw_get_database(database, key)
                .expect("captured point")
        })
        .collect();
    drop(overlay_scope);
    drop(scope);
    drop(owner);
    let fresh = cassie
        .midge
        .raw_scan_prefix_database(database, &[])
        .expect("fresh Data");
    let current = controls.current_query_memory_bytes();
    drop(cassie);
    std::fs::remove_dir_all(&path).expect("strict fixture cleanup");

    // Assert
    assert_ne!(fresh, before, "the UPDATE changed authoritative Data");
    assert_eq!(
        captured, before,
        "raw prefix shares the captured transaction"
    );
    assert_eq!(
        page, before,
        "raw pagination shares the captured transaction"
    );
    assert_eq!(
        point_values,
        before
            .iter()
            .map(|(_, value)| Some(value.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(current, 0);
}

#[test]
fn should_select_read_paths_using_the_captured_overlay() {
    // Arrange
    let path = std::env::temp_dir().join(format!("cassie-overlay-path-{}", uuid::Uuid::new_v4()));
    let cassie = Arc::new(Cassie::new_with_data_dir(&path).expect("Cassie"));
    cassie.startup().expect("startup");
    let reader = cassie.create_session("reader", None);
    execute(
        &cassie,
        &reader,
        "CREATE TABLE overlay_path_rows (id INT PRIMARY KEY, n INT)",
    );
    execute(
        &cassie,
        &reader,
        "INSERT INTO overlay_path_rows VALUES (1, 10)",
    );
    execute(
        &cassie,
        &reader,
        "CREATE INDEX overlay_path_column ON overlay_path_rows USING column (n)",
    );
    execute(&cassie, &reader, "BEGIN");
    execute(&cassie, &reader, "UPDATE overlay_path_rows SET n = 110");
    let nested_cassie = Arc::clone(&cassie);
    let nested_reader = reader.clone();
    let hook = CaptureHook::install(move || {
        execute(&nested_cassie, &nested_reader, "ROLLBACK");
    });

    // Act
    let captured = execute(
        &cassie,
        &reader,
        "SELECT n FROM overlay_path_rows ORDER BY n",
    );
    drop(hook);
    let fresh = execute(
        &cassie,
        &reader,
        "SELECT n FROM overlay_path_rows ORDER BY n",
    );
    drop(cassie);
    std::fs::remove_dir_all(&path).expect("strict fixture cleanup");

    // Assert
    assert_eq!(fresh, vec![vec![Value::Int64(10)]]);
    assert_eq!(
        captured,
        vec![vec![Value::Int64(110)]],
        "a live rollback cannot bypass the captured staged row"
    );
}
