use std::sync::{mpsc, Arc, Barrier};
use std::time::Duration;

const CONCURRENCY_DEADLINE: Duration = Duration::from_secs(5);

fn wait_for_barrier(barrier: &Arc<Barrier>, label: &str) {
    let (tx, rx) = mpsc::channel();
    let waiter = Arc::clone(barrier);
    let handle = std::thread::spawn(move || {
        waiter.wait();
        tx.send(()).expect("send barrier result");
    });
    rx.recv_timeout(CONCURRENCY_DEADLINE)
        .unwrap_or_else(|error| panic!("{label} barrier deadline: {error}"));
    handle
        .join()
        .unwrap_or_else(|_| panic!("{label} barrier waiter panicked"));
}

struct ProjectionPauseCleanup(Option<cassie::runtime::test_sync::TestBarrierCleanup>);

impl ProjectionPauseCleanup {
    fn arm(pairs: impl IntoIterator<Item = (Arc<Barrier>, Arc<Barrier>)>) -> Self {
        Self(Some(cassie::runtime::test_sync::TestBarrierCleanup::arm(
            pairs
                .into_iter()
                .flat_map(|(ready, resume)| [ready, resume]),
        )))
    }

    fn disarm(&mut self) {
        if let Some(mut cleanup) = self.0.take() {
            cleanup.disarm();
        }
    }
}

impl Drop for ProjectionPauseCleanup {
    fn drop(&mut self) {
        if self.0.is_none() {
            return;
        }
        set_projection_replay_prepare_barriers(None, None, None);
    }
}

use cassie::app::{
    projection_concurrency_test_guard, set_projection_replay_prepare_barriers, Cassie,
    CassieSession, ProjectionReplayBatch, ProjectionReplayEvent,
};
use cassie::executor::{
    set_materialized_projection_replace_barriers,
    set_materialized_projection_replace_start_barriers, set_projection_publication_wait_observer,
};
use cassie::types::Value;

fn data_dir(label: &str) -> String {
    crate::support_temp_dirs::data_path(&format!("projection-concurrency-{label}"))
        .to_string_lossy()
        .into_owned()
}

fn replay_batch(
    projection: &str,
    batch_id: &str,
    event_id: &str,
    checkpoint: &str,
    position: u64,
) -> ProjectionReplayBatch {
    ProjectionReplayBatch {
        projection: projection.to_string(),
        source_identity: "projection-concurrency-source".to_string(),
        batch_id: batch_id.to_string(),
        lag: 0,
        events: vec![ProjectionReplayEvent {
            event_id: event_id.to_string(),
            checkpoint: checkpoint.to_string(),
            position: Some(position),
            document_id: event_id.to_string(),
            payload: Some(serde_json::json!({"marker": checkpoint})),
        }],
    }
}

fn canonical_projection(cassie: &Cassie, collection: &str) -> String {
    cassie
        .catalog
        .get_schema(collection)
        .map_or_else(|| collection.to_string(), |schema| schema.collection)
}

fn execute_setup(cassie: &Cassie, session: &CassieSession, statements: &[&str]) {
    for sql in statements {
        cassie
            .execute_sql(session, sql, vec![])
            .expect("prepare projection fixture");
    }
}

fn join_projection_thread<T>(handle: std::thread::JoinHandle<T>, label: &str) {
    handle
        .join()
        .unwrap_or_else(|_| panic!("{label} thread panicked"));
}

fn concurrent_projection_fixture() -> (Arc<Cassie>, CassieSession) {
    let cassie = Arc::new(crate::support_sql::new_in_memory_cassie().expect("cassie"));
    cassie.startup().expect("startup");
    let session = cassie.create_session("setup", None);
    (cassie, session)
}

#[test]
fn should_serialize_concurrent_materialized_projection_refreshes() {
    // Arrange
    let _guard = projection_concurrency_test_guard();
    let (cassie, setup) = concurrent_projection_fixture();
    execute_setup(
        &cassie,
        &setup,
        &[
            "CREATE TABLE projection_concurrent_source (title TEXT, score INT)",
            "INSERT INTO projection_concurrent_source VALUES ('alpha', 1), ('bravo', 2)",
            "CREATE MATERIALIZED PROJECTION projection_concurrent AS \
             SELECT title FROM projection_concurrent_source WHERE score > 1",
            "CREATE MATERIALIZED PROJECTION projection_independent AS \
             SELECT title FROM projection_concurrent_source WHERE score = 1",
        ],
    );
    let dropped = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    let replace_ready = Arc::new(Barrier::new(2));
    let replace_resume = Arc::new(Barrier::new(2));
    let first_start = (Arc::clone(&replace_ready), Arc::clone(&replace_resume));
    let first_drop = (Arc::clone(&dropped), Arc::clone(&resume));
    let mut pause_cleanup = ProjectionPauseCleanup::arm([
        (Arc::clone(&replace_ready), Arc::clone(&replace_resume)),
        (Arc::clone(&dropped), Arc::clone(&resume)),
    ]);

    // Act
    let first_cassie = Arc::clone(&cassie);
    let (first_tx, first_rx) = mpsc::channel();
    let first = std::thread::spawn(move || {
        set_materialized_projection_replace_start_barriers(
            Some(first_start.0),
            Some(first_start.1),
        );
        set_materialized_projection_replace_barriers(Some(first_drop.0), Some(first_drop.1));
        let session = first_cassie.create_session("first", None);
        let result = first_cassie.execute_sql(
            &session,
            "REFRESH MATERIALIZED PROJECTION projection_concurrent",
            vec![],
        );
        first_tx.send(result).expect("send first refresh result");
    });
    wait_for_barrier(&replace_ready, "materialized projection replace ready");
    cassie
        .execute_sql(
            &setup,
            "INSERT INTO projection_concurrent_source VALUES ('charlie', 3)",
            vec![],
        )
        .expect("mutate source between refresh snapshots");
    cassie
        .execute_sql(
            &setup,
            "REFRESH MATERIALIZED PROJECTION projection_independent",
            vec![],
        )
        .expect("refresh an independent projection while the first snapshot is paused");
    wait_for_barrier(&replace_resume, "materialized projection replace resume");
    wait_for_barrier(&dropped, "materialized projection old relation dropped");
    let (second_tx, second_rx) = mpsc::channel();
    let (second_boundary_tx, second_boundary_rx) = mpsc::channel();
    let second_cassie = Arc::clone(&cassie);
    let second = std::thread::spawn(move || {
        let session = second_cassie.create_session("second", None);
        set_projection_publication_wait_observer(second_boundary_tx);
        let result = second_cassie.execute_sql(
            &session,
            "REFRESH MATERIALIZED PROJECTION projection_concurrent",
            vec![],
        );
        second_tx.send(result).expect("send second refresh result");
    });
    second_boundary_rx
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("second refresh contends on the logical projection publication gate");
    assert!(
        second_rx.try_recv().is_err(),
        "second refresh completed before publication"
    );
    wait_for_barrier(&resume, "materialized projection replacement publication");
    let first_result = first_rx
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("first refresh should complete");
    join_projection_thread(first, "first refresh");
    let second_result = second_rx
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("second refresh should complete");
    join_projection_thread(second, "second refresh");
    let rows = cassie
        .execute_sql(
            &setup,
            "SELECT title FROM projection_concurrent ORDER BY title",
            vec![],
        )
        .expect("query refreshed projection");

    // Assert
    first_result.expect("first refresh should succeed");
    second_result.expect("second refresh should succeed");
    assert_eq!(
        rows.rows(),
        vec![
            vec![Value::String("bravo".to_string())],
            vec![Value::String("charlie".to_string())],
        ]
    );
    pause_cleanup.disarm();
}

#[test]
fn should_wait_for_refresh_before_replay_acquires_collection_write_gates() {
    // Arrange
    let _guard = projection_concurrency_test_guard();
    let (cassie, setup) = concurrent_projection_fixture();
    execute_setup(
        &cassie,
        &setup,
        &[
            "CREATE TABLE mixed_source (title TEXT)",
            "INSERT INTO mixed_source VALUES ('original')",
            "CREATE MATERIALIZED PROJECTION mixed_target AS SELECT title FROM mixed_source",
            "CREATE MATERIALIZED PROJECTION mixed_other AS SELECT title FROM mixed_source",
        ],
    );
    let ready = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    let refresh_pause = (Arc::clone(&ready), Arc::clone(&resume));
    let mut cleanup = ProjectionPauseCleanup::arm([(Arc::clone(&ready), Arc::clone(&resume))]);
    let refresh_cassie = Arc::clone(&cassie);
    let refresh = std::thread::spawn(move || {
        set_materialized_projection_replace_start_barriers(
            Some(refresh_pause.0),
            Some(refresh_pause.1),
        );
        let session = refresh_cassie.create_session("refresh", None);
        refresh_cassie.execute_sql(
            &session,
            "REFRESH MATERIALIZED PROJECTION mixed_target",
            vec![],
        )
    });
    wait_for_barrier(&ready, "first refresh holds the publication gate");

    // Act
    let cancel = cassie::runtime::QueryCancellationHandle::new();
    let canceled = start_contending_replay(
        &cassie,
        "public.mixed_target",
        "canceled",
        Some(cancel.clone()),
    );
    cancel.cancel();
    let canceled_result = canceled
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("replay cancels before refresh resumes");
    let canonical = cassie.midge.canonical_collection_name("mixed_target");
    let after_cancel = cassie
        .midge
        .projection_metadata(&canonical)
        .unwrap()
        .unwrap();
    let replay = start_contending_replay(&cassie, &canonical, "completed", None);
    execute_setup(
        &cassie,
        &setup,
        &[
            "INSERT INTO mixed_source VALUES ('new')",
            "REFRESH MATERIALIZED PROJECTION mixed_other",
        ],
    );
    wait_for_barrier(&resume, "allow refresh to acquire output gates");
    let refresh_result = cassie::runtime::test_sync::join_test_thread(
        refresh,
        "refresh completes despite contended replay",
    );
    let replay_result = replay
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("replay reaches semantic validation");
    let durable = cassie
        .midge
        .projection_metadata(&canonical)
        .unwrap()
        .unwrap();
    let catalog = cassie.catalog.get_projection_metadata(&canonical).unwrap();

    // Assert
    assert!(matches!(
        canceled_result,
        Err(cassie::app::CassieError::QueryCanceled)
    ));
    assert_eq!(after_cancel.replay_batch_id, None);
    assert_eq!(after_cancel.last_error, None);
    refresh_result.expect("refresh publishes its output before replay");
    let expected_error = "projection replay event id cannot be empty";
    let Err(cassie::app::CassieError::Execution(message)) = replay_result else {
        panic!("expected replay validation error, got {replay_result:?}");
    };
    assert_eq!(message, expected_error);
    assert_eq!(durable.replay_batch_id.as_deref(), Some("completed"));
    assert_eq!(durable.last_error.as_deref(), Some(expected_error));
    assert_eq!(catalog.replay_batch_id, durable.replay_batch_id);
    assert_eq!(catalog.last_error, durable.last_error);
    cleanup.disarm();
}

fn start_contending_replay(
    cassie: &Arc<Cassie>,
    projection: &str,
    batch_id: &str,
    cancellation: Option<cassie::runtime::QueryCancellationHandle>,
) -> mpsc::Receiver<Result<cassie::app::ProjectionReplayReport, cassie::app::CassieError>> {
    let (wait_tx, wait_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let engine = Arc::clone(cassie);
    let mut batch = replay_batch(projection, batch_id, "document", "unused", 1);
    batch.source_identity = cassie
        .midge
        .projection_metadata(projection)
        .expect("read the contended projection")
        .expect("published projection")
        .source_identity
        .expect("materialized projection is bound to its input collection");
    batch.events[0].event_id.clear();
    std::thread::spawn(move || {
        set_projection_publication_wait_observer(wait_tx);
        let result = if let Some(cancel) = cancellation {
            engine.replay_projection_batch_cancellable(batch, &cancel)
        } else {
            engine.replay_projection_batch(batch)
        };
        done_tx.send(result).expect("send replay outcome");
    });
    wait_rx
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("actual same-projection publication contention");
    done_rx
}

#[test]
fn should_keep_active_materialized_view_readable_during_concurrent_refresh_build() {
    // Arrange
    let _guard = projection_concurrency_test_guard();
    let cassie = Arc::new(crate::support_sql::new_in_memory_cassie().expect("cassie"));
    cassie.startup().expect("startup");
    let setup = cassie.create_session("setup", None);
    cassie
        .execute_sql(
            &setup,
            "CREATE TABLE materialized_view_reader_source (id INT PRIMARY KEY, label TEXT)",
            vec![],
        )
        .expect("create source table");
    cassie
        .execute_sql(
            &setup,
            "INSERT INTO materialized_view_reader_source VALUES (1, 'one'), (2, 'two')",
            vec![],
        )
        .expect("seed source rows");
    cassie
        .execute_sql(
            &setup,
            "CREATE MATERIALIZED VIEW materialized_view_reader_copy (item_id, item_label) AS \
             SELECT id, label FROM materialized_view_reader_source",
            vec![],
        )
        .expect("create populated materialized view");
    cassie
        .execute_sql(
            &setup,
            "CREATE UNIQUE INDEX materialized_view_reader_copy_id_uq \
             ON materialized_view_reader_copy (item_id)",
            vec![],
        )
        .expect("create concurrent-refresh index");
    cassie
        .execute_sql(
            &setup,
            "UPDATE materialized_view_reader_source SET label = 'updated' WHERE id = 2",
            vec![],
        )
        .expect("update source before rebuilding the output");

    let build_ready = Arc::new(Barrier::new(2));
    let build_resume = Arc::new(Barrier::new(2));
    let build_pause = (Arc::clone(&build_ready), Arc::clone(&build_resume));
    let mut pause_cleanup =
        ProjectionPauseCleanup::arm([(Arc::clone(&build_ready), Arc::clone(&build_resume))]);
    let refresh_cassie = Arc::clone(&cassie);

    // Act
    let (refresh_tx, refresh_rx) = mpsc::channel();
    let refresh = std::thread::spawn(move || {
        set_materialized_projection_replace_start_barriers(
            Some(build_pause.0),
            Some(build_pause.1),
        );
        let session = refresh_cassie.create_session("refresh", None);
        let result = refresh_cassie.execute_sql(
            &session,
            "REFRESH MATERIALIZED VIEW CONCURRENTLY materialized_view_reader_copy",
            vec![],
        );
        refresh_tx.send(result).expect("send refresh result");
    });
    wait_for_barrier(&build_ready, "concurrent materialized view build ready");
    let during_build = cassie
        .execute_sql(
            &setup,
            "SELECT item_label FROM materialized_view_reader_copy ORDER BY item_id",
            vec![],
        )
        .expect("read the active output while the replacement is building");
    wait_for_barrier(&build_resume, "concurrent materialized view build resume");
    refresh_rx
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("concurrent refresh should complete")
        .expect("concurrent refresh should publish the replacement");
    join_projection_thread(refresh, "concurrent refresh");
    let after_refresh = cassie
        .execute_sql(
            &setup,
            "SELECT item_label FROM materialized_view_reader_copy ORDER BY item_id",
            vec![],
        )
        .expect("read the newly active output");

    // Assert
    assert_eq!(
        during_build.rows(),
        vec![
            vec![Value::String("one".to_string())],
            vec![Value::String("two".to_string())],
        ]
    );
    assert_eq!(
        after_refresh.rows(),
        vec![
            vec![Value::String("one".to_string())],
            vec![Value::String("updated".to_string())],
        ]
    );
    pause_cleanup.disarm();
}

#[test]
fn should_serialize_materialized_view_refresh_modes() {
    // Arrange
    let _guard = projection_concurrency_test_guard();
    let (cassie, setup) = concurrent_projection_fixture();
    execute_setup(
        &cassie,
        &setup,
        &[
            "CREATE TABLE materialized_view_refresh_order_source \
               (id INT PRIMARY KEY, label TEXT)",
            "INSERT INTO materialized_view_refresh_order_source VALUES (1, 'one')",
            "CREATE MATERIALIZED VIEW materialized_view_refresh_order_copy \
               (item_id, item_label) AS \
               SELECT id, label FROM materialized_view_refresh_order_source",
            "CREATE UNIQUE INDEX materialized_view_refresh_order_copy_id_uq \
               ON materialized_view_refresh_order_copy (item_id)",
            "UPDATE materialized_view_refresh_order_source \
               SET label = 'concurrent' WHERE id = 1",
        ],
    );
    let build_ready = Arc::new(Barrier::new(2));
    let build_resume = Arc::new(Barrier::new(2));
    let build_pause = (Arc::clone(&build_ready), Arc::clone(&build_resume));
    let mut pause_cleanup =
        ProjectionPauseCleanup::arm([(Arc::clone(&build_ready), Arc::clone(&build_resume))]);
    let concurrent_cassie = Arc::clone(&cassie);

    // Act
    let (concurrent_tx, concurrent_rx) = mpsc::channel();
    let concurrent = std::thread::spawn(move || {
        set_materialized_projection_replace_start_barriers(
            Some(build_pause.0),
            Some(build_pause.1),
        );
        let session = concurrent_cassie.create_session("concurrent", None);
        let result = concurrent_cassie.execute_sql(
            &session,
            "REFRESH MATERIALIZED VIEW CONCURRENTLY materialized_view_refresh_order_copy",
            vec![],
        );
        concurrent_tx
            .send(result)
            .expect("send concurrent refresh result");
    });
    wait_for_barrier(&build_ready, "refresh mode build ready");
    cassie
        .execute_sql(
            &setup,
            "UPDATE materialized_view_refresh_order_source SET label = 'ordinary' WHERE id = 1",
            vec![],
        )
        .expect("change source before the ordinary refresh");
    let (started_tx, started_rx) = mpsc::channel();
    let (ordinary_tx, ordinary_rx) = mpsc::channel();
    let ordinary_cassie = Arc::clone(&cassie);
    let ordinary = std::thread::spawn(move || {
        let session = ordinary_cassie.create_session("ordinary", None);
        started_tx.send(()).expect("signal ordinary refresh start");
        ordinary_tx
            .send(ordinary_cassie.execute_sql(
                &session,
                "REFRESH MATERIALIZED VIEW materialized_view_refresh_order_copy",
                vec![],
            ))
            .expect("send ordinary refresh result");
    });
    assert_eq!(
        started_rx
            .recv_timeout(CONCURRENCY_DEADLINE)
            .expect("ordinary refresh reaches execution boundary"),
        ()
    );
    wait_for_barrier(&build_resume, "refresh mode build resume");
    let concurrent_result = concurrent_rx
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("concurrent refresh should complete");
    join_projection_thread(concurrent, "concurrent refresh");
    let ordinary_result = ordinary_rx
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("ordinary refresh should complete after publication");
    join_projection_thread(ordinary, "ordinary refresh");
    let rows = cassie
        .execute_sql(
            &setup,
            "SELECT item_label FROM materialized_view_refresh_order_copy",
            vec![],
        )
        .expect("read the final materialized-view output");
    let has_indexes = cassie
        .execute_sql(
            &setup,
            "SELECT hasindexes FROM pg_matviews \
             WHERE matviewname = 'materialized_view_refresh_order_copy'",
            vec![],
        )
        .expect("read final materialized-view index state");

    // Assert
    concurrent_result.expect("concurrent refresh should succeed");
    ordinary_result.expect("ordinary refresh should succeed");
    assert_eq!(
        rows.rows(),
        vec![vec![Value::String("ordinary".to_string())]]
    );
    assert_eq!(has_indexes.rows(), vec![vec![Value::Bool(true)]]);
    pause_cleanup.disarm();
}

#[test]
fn should_rollback_concurrent_materialized_view_refresh_with_transaction() {
    // Arrange
    let cassie = crate::support_sql::new_in_memory_cassie().expect("cassie");
    cassie.startup().expect("startup");
    let setup = cassie.create_session("setup", None);
    let transaction = cassie.create_session("transaction", None);
    let observer = cassie.create_session("observer", None);
    for sql in [
        "CREATE TABLE transactional_materialized_view_source \
           (id INT PRIMARY KEY, label TEXT)",
        "INSERT INTO transactional_materialized_view_source VALUES (1, 'one'), (2, 'two')",
        "CREATE MATERIALIZED VIEW transactional_materialized_view_copy \
           (item_id, item_label) AS \
           SELECT id, label FROM transactional_materialized_view_source",
        "CREATE UNIQUE INDEX transactional_materialized_view_copy_id_uq \
           ON transactional_materialized_view_copy (item_id)",
        "UPDATE transactional_materialized_view_source \
           SET label = 'updated' WHERE id = 2",
    ] {
        cassie
            .execute_sql(&setup, sql, vec![])
            .unwrap_or_else(|error| panic!("execute setup SQL '{sql}': {error}"));
    }

    // Act
    cassie
        .execute_sql(&transaction, "BEGIN", vec![])
        .expect("begin transaction");
    cassie
        .execute_sql(
            &transaction,
            "REFRESH MATERIALIZED VIEW CONCURRENTLY transactional_materialized_view_copy",
            vec![],
        )
        .expect("refresh concurrently inside explicit transaction");
    let transaction_rows = cassie
        .execute_sql(
            &transaction,
            "SELECT item_label FROM transactional_materialized_view_copy ORDER BY item_id",
            vec![],
        )
        .expect("read the transaction-local refreshed output");
    let observer_rows = cassie
        .execute_sql(
            &observer,
            "SELECT item_label FROM transactional_materialized_view_copy ORDER BY item_id",
            vec![],
        )
        .expect("read the committed output from another session");
    cassie
        .execute_sql(&transaction, "ROLLBACK", vec![])
        .expect("roll back concurrent refresh");
    let rolled_back_rows = cassie
        .execute_sql(
            &transaction,
            "SELECT item_label FROM transactional_materialized_view_copy ORDER BY item_id",
            vec![],
        )
        .expect("read the output after rollback");

    // Assert
    assert_eq!(
        transaction_rows.rows(),
        vec![
            vec![Value::String("one".to_string())],
            vec![Value::String("updated".to_string())],
        ]
    );
    assert_eq!(
        observer_rows.rows(),
        vec![
            vec![Value::String("one".to_string())],
            vec![Value::String("two".to_string())],
        ]
    );
    assert_eq!(rolled_back_rows.rows(), observer_rows.rows());
}

#[test]
fn should_keep_concurrent_replay_checkpoint_monotonic() {
    // Arrange
    let _guard = projection_concurrency_test_guard();
    let path = data_dir("replay-checkpoint");
    let cassie = Arc::new(crate::support_sql::new_in_memory_cassie().expect("cassie"));
    cassie.startup().expect("startup");
    let setup = cassie.create_session("setup", None);
    cassie
        .execute_sql(
            &setup,
            "CREATE TABLE projection_checkpoint_target (marker TEXT)",
            vec![],
        )
        .expect("create replay target");
    let projection = canonical_projection(&cassie, "projection_checkpoint_target");
    let ready = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    set_projection_replay_prepare_barriers(
        Some("batch-low".to_string()),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );
    let mut pause_cleanup =
        ProjectionPauseCleanup::arm([(Arc::clone(&ready), Arc::clone(&resume))]);

    // Act
    let (low_tx, low_rx) = mpsc::channel();
    let low_cassie = Arc::clone(&cassie);
    let low_projection = projection.clone();
    let low = std::thread::spawn(move || {
        let result = low_cassie.replay_projection_batch(replay_batch(
            &low_projection,
            "batch-low",
            "event-low",
            "checkpoint-5",
            5,
        ));
        low_tx.send(result).expect("send low replay result");
    });
    wait_for_barrier(&ready, "low replay prepare ready");
    let (high_tx, high_rx) = mpsc::channel();
    let (high_boundary_tx, high_boundary_rx) = mpsc::channel();
    let high_cassie = Arc::clone(&cassie);
    let high_projection = projection.clone();
    let high = std::thread::spawn(move || {
        high_boundary_tx
            .send(())
            .expect("signal high replay boundary");
        let result = high_cassie.replay_projection_batch(replay_batch(
            &high_projection,
            "batch-high",
            "event-high",
            "checkpoint-10",
            10,
        ));
        high_tx.send(result).expect("send high replay result");
    });
    high_boundary_rx
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("high replay reaches execution boundary");
    assert!(
        high_rx.try_recv().is_err(),
        "high replay completed before low replay resumed"
    );
    wait_for_barrier(&resume, "low replay prepare resume");
    low_rx
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("low replay should complete")
        .expect("low replay");
    low.join()
        .unwrap_or_else(|_| panic!("low replay thread panicked"));
    let high_result = high_rx
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("high replay should complete");
    high.join()
        .unwrap_or_else(|_| panic!("high replay thread panicked"));
    high_result.expect("high replay");
    let metadata = cassie
        .catalog
        .get_projection_metadata(&projection)
        .expect("projection metadata");

    // Assert
    assert_eq!(metadata.source_position, Some(10));
    assert_eq!(metadata.source_checkpoint.as_deref(), Some("checkpoint-10"));
    pause_cleanup.disarm();
    let _ = std::fs::remove_dir_all(path);
}

#[test]
fn should_apply_concurrent_duplicate_replay_event_once() {
    // Arrange
    let _guard = projection_concurrency_test_guard();
    let path = data_dir("replay-duplicate");
    let cassie = Arc::new(crate::support_sql::new_in_memory_cassie().expect("cassie"));
    cassie.startup().expect("startup");
    let setup = cassie.create_session("setup", None);
    cassie
        .execute_sql(
            &setup,
            "CREATE TABLE projection_duplicate_target (marker TEXT)",
            vec![],
        )
        .expect("create replay target");
    let projection = canonical_projection(&cassie, "projection_duplicate_target");
    let ready = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    set_projection_replay_prepare_barriers(
        Some("batch-paused".to_string()),
        Some(Arc::clone(&ready)),
        Some(Arc::clone(&resume)),
    );
    let mut pause_cleanup =
        ProjectionPauseCleanup::arm([(Arc::clone(&ready), Arc::clone(&resume))]);

    // Act
    let paused_cassie = Arc::clone(&cassie);
    let paused_projection = projection.clone();
    let (paused_tx, paused_rx) = mpsc::channel();
    let paused = std::thread::spawn(move || {
        let result = paused_cassie.replay_projection_batch(replay_batch(
            &paused_projection,
            "batch-paused",
            "event-shared",
            "checkpoint-1",
            1,
        ));
        paused_tx.send(result).expect("send paused replay result");
    });
    wait_for_barrier(&ready, "duplicate replay prepare ready");
    let (racing_tx, racing_rx) = mpsc::channel();
    let (racing_boundary_tx, racing_boundary_rx) = mpsc::channel();
    let racing_cassie = Arc::clone(&cassie);
    let racing_projection = projection.clone();
    let racing = std::thread::spawn(move || {
        racing_boundary_tx
            .send(())
            .expect("signal racing replay boundary");
        let result = racing_cassie.replay_projection_batch(replay_batch(
            &racing_projection,
            "batch-racing",
            "event-shared",
            "checkpoint-1",
            1,
        ));
        racing_tx.send(result).expect("send racing replay result");
    });
    racing_boundary_rx
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("racing replay reaches execution boundary");
    assert!(
        racing_rx.try_recv().is_err(),
        "racing replay completed before paused replay resumed"
    );
    wait_for_barrier(&resume, "duplicate replay prepare resume");
    let paused_result = paused_rx
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("paused replay should complete")
        .expect("paused replay");
    let paused_handle = paused;
    paused_handle
        .join()
        .unwrap_or_else(|_| panic!("paused replay thread panicked"));
    let racing_result = racing_rx
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("racing replay should complete")
        .expect("racing replay");
    let racing_handle = racing;
    racing_handle
        .join()
        .unwrap_or_else(|_| panic!("racing replay thread panicked"));

    // Assert
    assert_eq!(
        paused_result.applied_event_count + racing_result.applied_event_count,
        1
    );
    assert_eq!(
        paused_result.skipped_duplicate_count + racing_result.skipped_duplicate_count,
        1
    );
    pause_cleanup.disarm();
    let _ = std::fs::remove_dir_all(path);
}
