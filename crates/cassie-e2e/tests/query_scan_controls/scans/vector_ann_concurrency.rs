use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Barrier,
};
use std::time::Duration;

const CONCURRENCY_DEADLINE: Duration = Duration::from_secs(5);

fn wait_for_barrier(barrier: &Arc<Barrier>, label: &str, consumed: Option<&AtomicBool>) {
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
    if let Some(consumed) = consumed {
        consumed.store(true, Ordering::SeqCst);
    }
}

struct VectorPauseCleanup {
    cassie: Option<Arc<cassie::app::Cassie>>,
    resume: Arc<Barrier>,
    selected_consumed: Arc<AtomicBool>,
    resume_released: Arc<AtomicBool>,
    armed: bool,
}

impl VectorPauseCleanup {
    fn arm(
        cassie: &Arc<cassie::app::Cassie>,
        _selected: Arc<Barrier>,
        resume: Arc<Barrier>,
        selected_consumed: Arc<AtomicBool>,
        resume_released: Arc<AtomicBool>,
    ) -> Self {
        Self {
            cassie: Some(Arc::clone(cassie)),
            resume,
            selected_consumed,
            resume_released,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for VectorPauseCleanup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        if let Some(cassie) = self.cassie.take() {
            cassie::executor::set_vector_ann_rerank_barriers(&cassie, None, None);
        }
        if self.selected_consumed.load(Ordering::SeqCst)
            && !self.resume_released.load(Ordering::SeqCst)
        {
            let resume = Arc::clone(&self.resume);
            let (tx, rx) = mpsc::channel();
            let handle = std::thread::spawn(move || {
                resume.wait();
                tx.send(()).expect("send ANN resume cleanup");
            });
            let _ = rx.recv_timeout(CONCURRENCY_DEADLINE);
            let _ = handle.join();
            self.resume_released.store(true, Ordering::SeqCst);
        }
    }
}

use cassie::app::Cassie;
use cassie::config::{
    CassieRuntimeConfig, CassieRuntimeLimits, EmbeddingsRuntimeConfig, ExecutionResultCacheEnabled,
    LocalRuntimeConfig,
};

const TABLE: &str = "ann_concurrent_source";
const QUERY: &str = "SELECT id, vector_distance(embedding, '[0,0,0]') AS distance FROM ann_concurrent_source ORDER BY distance ASC LIMIT 5";

fn fixture(index_type: &str) -> Arc<Cassie> {
    fixture_before_index(index_type, |_| {})
}

fn fixture_before_index(index_type: &str, before_index: impl FnOnce(&Cassie)) -> Arc<Cassie> {
    // Setup uses its ordinary creation budget. Loading probes narrow only the
    // actual executor controls, after the table/index and plan exist.
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "deterministic-test".to_string(),
        dimensions: 3,
    });
    let cassie =
        Arc::new(Cassie::new_in_memory_with_config(config).expect("create concurrent ANN fixture"));
    crate::support_sql::install_vector_extension(&cassie);
    cassie.startup().expect("startup");
    let session = cassie.create_session("tester", None);
    cassie
        .execute_sql(
            &session,
            "CREATE TABLE ann_concurrent_source (content TEXT, embedding VECTOR(3))",
            vec![],
        )
        .expect("create table");
    let documents = (0..32)
        .map(|index| {
            let coordinate = index.to_string().parse::<f64>().expect("coordinate") / 100.0;
            (
                Some(format!("row-{index:04}")),
                serde_json::json!({
                    "content": format!("row-{index:04}"),
                    "embedding": [coordinate, coordinate / 2.0, 0.0]
                }),
            )
        })
        .collect();
    cassie
        .midge
        .put_fresh_documents(TABLE, documents)
        .expect("seed rows");
    before_index(&cassie);
    let options = match index_type {
        "hnsw" => "index_type = hnsw, m = 8, ef_construction = 64, ef_search = 32",
        "ivfflat" => "index_type = ivfflat, lists = 4, probes = 4, training_sample_size = 32, training_seed = 7",
        _ => panic!("unsupported fixture index type"),
    };
    cassie
        .execute_sql(
            &session,
            &format!("CREATE INDEX ann_concurrent_vector ON ann_concurrent_source USING vector (embedding) WITH (source_field = content, metric = l2, {options})"),
            vec![],
        )
        .expect("create HNSW index");
    cassie
}

fn exact_after_dropping_index(cassie: &Cassie) -> Vec<Vec<cassie::types::Value>> {
    cassie
        .execute_sql(
            &cassie.create_session("tester", None),
            "DROP INDEX ann_concurrent_vector ON ann_concurrent_source",
            vec![],
        )
        .expect("drop ANN index for exact baseline");
    let result = cassie
        .execute_sql(&cassie.create_session("tester", None), QUERY, vec![])
        .expect("exact baseline");
    result.rows().to_vec()
}

#[test]
fn should_preserve_captured_hnsw_source_when_deleted_before_reranking() {
    // Arrange
    let _hook_guard = cassie::midge::adapter::query_scan_control_test_guard();
    let mut expected = None;
    let cassie = fixture_before_index("hnsw", |cassie| {
        expected = Some(
            cassie
                .execute_sql(&cassie.create_session("baseline", None), QUERY, vec![])
                .expect("pre-index exact source oracle"),
        );
    });
    let expected = expected.expect("captured exact source oracle");
    let before = cassie.metrics();
    let selected = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    let selected_consumed = Arc::new(AtomicBool::new(false));
    let resume_released = Arc::new(AtomicBool::new(false));
    cassie::executor::set_vector_ann_rerank_barriers(
        &cassie,
        Some(Arc::clone(&selected)),
        Some(Arc::clone(&resume)),
    );
    let mut pause_cleanup = VectorPauseCleanup::arm(
        &cassie,
        Arc::clone(&selected),
        Arc::clone(&resume),
        Arc::clone(&selected_consumed),
        Arc::clone(&resume_released),
    );
    let query_cassie = Arc::clone(&cassie);
    let (result_tx, result_rx) = mpsc::channel();
    let query = std::thread::spawn(move || {
        let result = query_cassie
            .execute_sql(&query_cassie.create_session("reader", None), QUERY, vec![])
            .expect("concurrent ANN query");
        result_tx.send(result).expect("send HNSW query result");
    });
    wait_for_barrier(
        &selected,
        "HNSW rerank candidate selection",
        Some(&selected_consumed),
    );

    // Act
    cassie
        .execute_sql(
            &cassie.create_session("writer", None),
            "DELETE FROM ann_concurrent_source WHERE id = 'row-0000'",
            vec![],
        )
        .expect("delete selected source row");
    wait_for_barrier(&resume, "HNSW rerank resume", Some(&resume_released));
    let resolved = result_rx
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("HNSW rerank query result");
    query
        .join()
        .unwrap_or_else(|_| panic!("HNSW rerank query thread panicked"));
    let reader_metrics = cassie.metrics();
    let exact = exact_after_dropping_index(&cassie);

    // Assert
    assert_eq!(
        resolved.rows(),
        expected.rows(),
        "paused reader keeps its source view"
    );
    assert_ne!(
        resolved.rows(),
        exact,
        "next statement sees the committed mutation"
    );
    assert_eq!(
        resolved.rows()[0][0],
        cassie::types::Value::String("row-0000".into())
    );
    assert_eq!(resolved.rows()[0][1], cassie::types::Value::Float64(0.0));
    assert!(exact
        .iter()
        .all(|row| row[0] != cassie::types::Value::String("row-0000".into())));
    assert_ne!(
        reader_metrics["vector"]["last_fallback_reason"].as_str(),
        Some("concurrent-source-change")
    );
    assert_eq!(
        reader_metrics["vector"]["hnsw_executions"].as_u64(),
        before["vector"]["hnsw_executions"]
            .as_u64()
            .map(|count| count + 1)
    );
    assert!(
        reader_metrics["vector"]["ann_reads_total"]
            .as_u64()
            .unwrap()
            > before["vector"]["ann_reads_total"].as_u64().unwrap()
    );
    pause_cleanup.disarm();
    drop(expected);
    drop(resolved);
    drop(exact);
    cassie.shutdown();
    drop(cassie);
}

#[test]
fn should_preserve_captured_ivfflat_source_when_replaced_before_reranking() {
    // Arrange
    let _hook_guard = cassie::midge::adapter::query_scan_control_test_guard();
    let mut expected = None;
    let cassie = fixture_before_index("ivfflat", |cassie| {
        expected = Some(
            cassie
                .execute_sql(&cassie.create_session("baseline", None), QUERY, vec![])
                .expect("pre-index exact source oracle"),
        );
    });
    let expected = expected.expect("captured exact source oracle");
    let before = cassie.metrics();
    let selected = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    let selected_consumed = Arc::new(AtomicBool::new(false));
    let resume_released = Arc::new(AtomicBool::new(false));
    cassie::executor::set_vector_ann_rerank_barriers(
        &cassie,
        Some(Arc::clone(&selected)),
        Some(Arc::clone(&resume)),
    );
    let mut pause_cleanup = VectorPauseCleanup::arm(
        &cassie,
        Arc::clone(&selected),
        Arc::clone(&resume),
        Arc::clone(&selected_consumed),
        Arc::clone(&resume_released),
    );
    let query_cassie = Arc::clone(&cassie);
    let (result_tx, result_rx) = mpsc::channel();
    let query = std::thread::spawn(move || {
        let result = query_cassie
            .execute_sql(&query_cassie.create_session("reader", None), QUERY, vec![])
            .expect("concurrent IVFFlat query");
        result_tx.send(result).expect("send IVFFlat query result");
    });
    wait_for_barrier(
        &selected,
        "IVFFlat rerank candidate selection",
        Some(&selected_consumed),
    );

    // Act
    cassie
        .execute_sql(
            &cassie.create_session("writer", None),
            "UPDATE ann_concurrent_source SET embedding = '[9,9,9]' WHERE id = 'row-0000'",
            vec![],
        )
        .expect("replace selected source vector");
    wait_for_barrier(&resume, "IVFFlat rerank resume", Some(&resume_released));
    let resolved = result_rx
        .recv_timeout(CONCURRENCY_DEADLINE)
        .expect("IVFFlat rerank query result");
    query
        .join()
        .unwrap_or_else(|_| panic!("IVFFlat rerank query thread panicked"));
    let reader_metrics = cassie.metrics();
    let exact = exact_after_dropping_index(&cassie);

    // Assert
    assert_eq!(
        resolved.rows(),
        expected.rows(),
        "paused reader keeps its source view"
    );
    assert_ne!(
        resolved.rows(),
        exact,
        "next statement sees the committed mutation"
    );
    assert_eq!(
        resolved.rows()[0][0],
        cassie::types::Value::String("row-0000".into())
    );
    assert_eq!(resolved.rows()[0][1], cassie::types::Value::Float64(0.0));
    assert!(exact
        .iter()
        .all(|row| row[0] != cassie::types::Value::String("row-0000".into())));
    assert_ne!(
        reader_metrics["vector"]["last_fallback_reason"].as_str(),
        Some("concurrent-source-change")
    );
    assert_eq!(
        reader_metrics["vector"]["ivfflat_executions"].as_u64(),
        before["vector"]["ivfflat_executions"]
            .as_u64()
            .map(|count| count + 1)
    );
    assert!(
        reader_metrics["vector"]["ann_reads_total"]
            .as_u64()
            .unwrap()
            > before["vector"]["ann_reads_total"].as_u64().unwrap()
    );
    pause_cleanup.disarm();
    drop(expected);
    drop(resolved);
    drop(exact);
    cassie.shutdown();
    drop(cassie);
}

#[test]
fn should_pause_only_the_installing_fixture_at_the_ann_rerank_boundary() {
    for index_type in ["hnsw", "ivfflat"] {
        // Arrange
        let owner = fixture(index_type);
        let unrelated = fixture(index_type);
        let selected = Arc::new(Barrier::new(1));
        let resume = Arc::new(Barrier::new(1));
        cassie::executor::set_vector_ann_rerank_barriers(
            &owner,
            Some(Arc::clone(&selected)),
            Some(Arc::clone(&resume)),
        );

        // Act
        let unrelated_rows = unrelated
            .execute_sql(&unrelated.create_session("reader", None), QUERY, vec![])
            .expect("unrelated ANN query")
            .rows()
            .len();
        let installed_after_unrelated = Arc::strong_count(&selected);
        let owner_rows = owner
            .execute_sql(&owner.create_session("reader", None), QUERY, vec![])
            .expect("owner ANN query")
            .rows()
            .len();
        let installed_after_owner = Arc::strong_count(&selected);

        // Assert
        assert_eq!(unrelated_rows, 5, "{index_type} unrelated rows");
        assert_eq!(
            installed_after_unrelated, 2,
            "{index_type} unrelated fixture consumed another fixture's rerank barriers"
        );
        assert_eq!(owner_rows, 5, "{index_type} owner rows");
        assert_eq!(
            installed_after_owner, 1,
            "{index_type} owner fixture did not consume its rerank barriers"
        );
        for cassie in [owner, unrelated] {
            cassie.shutdown();
            drop(cassie);
        }
    }
}

#[test]
fn should_cancel_hnsw_candidate_loading_without_publishing_success_metrics() {
    // Arrange
    let _guard = cassie::midge::adapter::query_scan_control_test_guard();
    let cassie = fixture("hnsw");
    let before = cassie.metrics();
    cassie::midge::adapter::set_query_scan_cancellation_after_entries(Some(3));

    // Act
    let error = cassie
        .execute_sql(&cassie.create_session("reader", None), QUERY, vec![])
        .expect_err("controlled HNSW loading should observe cancellation");
    let after = cassie.metrics();

    // Assert
    assert!(
        error.to_string().contains("cancel"),
        "unexpected cancellation error: {error}"
    );
    assert_eq!(
        after["vector"]["hnsw_executions"],
        before["vector"]["hnsw_executions"]
    );
    assert_eq!(after["vector"]["count"], before["vector"]["count"]);
    assert_eq!(
        after["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}

#[test]
fn should_cancel_ivfflat_membership_loading_without_publishing_success_metrics() {
    // Arrange
    let _guard = cassie::midge::adapter::query_scan_control_test_guard();
    let cassie = fixture("ivfflat");
    let before = cassie.metrics();
    cassie::midge::adapter::set_query_scan_cancellation_after_entries(Some(3));

    // Act
    let error = cassie
        .execute_sql(&cassie.create_session("reader", None), QUERY, vec![])
        .expect_err("controlled IVFFlat membership loading should observe cancellation");
    let after = cassie.metrics();

    // Assert
    assert!(
        error.to_string().contains("cancel"),
        "unexpected cancellation error: {error}"
    );
    assert_eq!(
        after["vector"]["ivfflat_executions"],
        before["vector"]["ivfflat_executions"]
    );
    assert_eq!(after["vector"]["count"], before["vector"]["count"]);
    assert_eq!(
        after["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}

#[test]
fn should_reject_hnsw_loading_when_query_memory_is_exhausted() {
    // Arrange
    let _hook_guard = cassie::midge::adapter::query_scan_control_test_guard();
    let cassie = fixture("hnsw");
    let plan = cassie
        .compile_sql_physical_plan_for_diagnostics(QUERY)
        .expect("compile ANN plan under creation budget");
    let ordinary_limits = CassieRuntimeLimits::default();
    let ordinary = cassie::runtime::QueryExecutionControls::from_limits(
        &ordinary_limits,
        std::time::Instant::now(),
    );
    let baseline = cassie.metrics();
    let positive = cassie::executor::run_with_controls(&cassie, &plan, vec![], &ordinary)
        .expect("same executor plan selects ANN with ordinary controls");
    assert_eq!(positive.rows().len(), 5);
    assert_eq!(
        cassie.metrics()["vector"]["hnsw_executions"].as_u64(),
        baseline["vector"]["hnsw_executions"]
            .as_u64()
            .map(|count| count + 1)
    );
    drop(positive);
    assert_eq!(ordinary.current_query_memory_bytes(), 0);
    let mut restricted_limits = ordinary_limits;
    restricted_limits.query_memory_budget_bytes = 1_024;
    let restricted = cassie::runtime::QueryExecutionControls::from_limits(
        &restricted_limits,
        std::time::Instant::now(),
    );
    let before = cassie.metrics();

    // Act: this is the executor-loading resource boundary, not end-to-end SQL admission.
    let error = cassie::executor::run_with_controls(&cassie, &plan, vec![], &restricted)
        .expect_err("accounted HNSW state should exceed the low query budget");
    let after = cassie.metrics();

    // Assert
    assert_eq!(restricted.query_memory_budget_bytes, 1_024);
    assert_eq!(restricted.current_query_memory_bytes(), 0);
    assert!(
        error.to_string().contains("query memory budget"),
        "unexpected memory error: {error}"
    );
    assert_eq!(
        after["vector"]["hnsw_executions"],
        before["vector"]["hnsw_executions"]
    );
    assert_eq!(after["vector"]["count"], before["vector"]["count"]);
    assert_eq!(
        after["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}

#[test]
fn should_publish_hnsw_metrics_only_after_selecting_the_ann_path() {
    // Arrange
    let _hook_guard = cassie::midge::adapter::query_scan_control_test_guard();
    let cassie = fixture("hnsw");
    let before = cassie.metrics();

    // Act
    let result = cassie
        .execute_sql(&cassie.create_session("reader", None), QUERY, vec![])
        .expect("HNSW query");
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows().len(), 5);
    assert_eq!(
        after["vector"]["hnsw_executions"].as_u64(),
        before["vector"]["hnsw_executions"]
            .as_u64()
            .map(|value| value + 1)
    );
    assert!(
        after["vector"]["ann_reads_total"].as_u64().unwrap()
            > before["vector"]["ann_reads_total"].as_u64().unwrap()
    );
    assert!(
        after["vector"]["exact_reranks_total"].as_u64().unwrap()
            > before["vector"]["exact_reranks_total"].as_u64().unwrap()
    );
    drop(result);
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}

#[test]
fn should_reject_ivfflat_loading_when_query_memory_is_exhausted() {
    // Arrange
    let _hook_guard = cassie::midge::adapter::query_scan_control_test_guard();
    let cassie = fixture("ivfflat");
    let plan = cassie
        .compile_sql_physical_plan_for_diagnostics(QUERY)
        .expect("compile ANN plan under creation budget");
    let ordinary_limits = CassieRuntimeLimits::default();
    let ordinary = cassie::runtime::QueryExecutionControls::from_limits(
        &ordinary_limits,
        std::time::Instant::now(),
    );
    let baseline = cassie.metrics();
    let positive = cassie::executor::run_with_controls(&cassie, &plan, vec![], &ordinary)
        .expect("same executor plan selects ANN with ordinary controls");
    assert_eq!(positive.rows().len(), 5);
    assert_eq!(
        cassie.metrics()["vector"]["ivfflat_executions"].as_u64(),
        baseline["vector"]["ivfflat_executions"]
            .as_u64()
            .map(|count| count + 1)
    );
    drop(positive);
    assert_eq!(ordinary.current_query_memory_bytes(), 0);
    let mut restricted_limits = ordinary_limits;
    restricted_limits.query_memory_budget_bytes = 1_024;
    let restricted = cassie::runtime::QueryExecutionControls::from_limits(
        &restricted_limits,
        std::time::Instant::now(),
    );
    let before = cassie.metrics();

    // Act: this is the executor-loading resource boundary, not end-to-end SQL admission.
    let error = cassie::executor::run_with_controls(&cassie, &plan, vec![], &restricted)
        .expect_err("accounted IVFFlat state should exceed the low query budget");
    let after = cassie.metrics();

    // Assert
    assert_eq!(restricted.query_memory_budget_bytes, 1_024);
    assert_eq!(restricted.current_query_memory_bytes(), 0);
    assert!(
        error.to_string().contains("query memory budget"),
        "unexpected memory error: {error}"
    );
    assert_eq!(
        after["vector"]["ivfflat_executions"],
        before["vector"]["ivfflat_executions"]
    );
    assert_eq!(after["vector"]["count"], before["vector"]["count"]);
    assert_eq!(
        after["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}

#[test]
fn should_publish_ivfflat_metrics_only_after_selecting_the_ann_path() {
    // Arrange
    let _hook_guard = cassie::midge::adapter::query_scan_control_test_guard();
    let cassie = fixture("ivfflat");
    let before = cassie.metrics();

    // Act
    let result = cassie
        .execute_sql(&cassie.create_session("reader", None), QUERY, vec![])
        .expect("IVFFlat query");
    let after = cassie.metrics();

    // Assert
    assert_eq!(result.rows().len(), 5);
    assert_eq!(
        after["vector"]["ivfflat_executions"].as_u64(),
        before["vector"]["ivfflat_executions"]
            .as_u64()
            .map(|value| value + 1)
    );
    assert!(
        after["vector"]["ann_reads_total"].as_u64().unwrap()
            > before["vector"]["ann_reads_total"].as_u64().unwrap()
    );
    assert!(
        after["vector"]["candidate_row_fetches_total"]
            .as_u64()
            .unwrap()
            > before["vector"]["candidate_row_fetches_total"]
                .as_u64()
                .unwrap()
    );
    drop(result);
    assert_eq!(
        cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
        Some(0)
    );
}
