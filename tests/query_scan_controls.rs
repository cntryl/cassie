// Integration suites that exercise the process-global query scan test hook.
// Keeping them in one flat test target prevents unrelated scans from consuming
// an armed hook while preserving parallel setup through shared test guards.

#[path = "support/graph_evidence.rs"]
mod support_graph_evidence;
#[path = "support/pgwire.rs"]
mod support_pgwire;
#[path = "support/sql.rs"]
mod support_sql;
#[path = "support/temp_dirs.rs"]
mod support_temp_dirs;

// Formerly tests/column_batch_controls.rs.
mod column_batch_controls {
    use cassie::app::{Cassie, CassieError};
    use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};

    use super::support_sql as support;

    const COLLECTION: &str = "controlled_column_metadata";
    const METADATA_REJECTION_BUDGET: usize = 1_024;

    struct Fixture {
        cassie: Cassie,
        path: String,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn fixture() -> Fixture {
        support::use_local_storage();
        let path = support::data_dir("column-metadata-controls");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_memory_budget_bytes = METADATA_REJECTION_BUDGET;
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, config).expect("controlled cassie");
        cassie.startup().expect("startup controlled cassie");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE {COLLECTION} (score INT, label TEXT)"),
                vec![],
            )
            .expect("create controlled table");
        let rows = (0..64)
            .map(|index| {
                (
                    Some(format!("row-{index:04}")),
                    serde_json::json!({
                        "score": index,
                        "label": format!("label-{index:04}-{}", "x".repeat(512)),
                    }),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_documents(COLLECTION, rows)
            .expect("seed controlled rows");
        cassie
            .execute_sql(
                &session,
                &format!(
                    "CREATE INDEX controlled_column_metadata_idx ON {COLLECTION} USING column \
                 (score, label) WITH (segment_size = 1)"
                ),
                vec![],
            )
            .expect("create metadata-heavy column index");
        Fixture { cassie, path }
    }

    fn metric(metrics: &serde_json::Value, family: &str, name: &str) -> u64 {
        metrics[family][name].as_u64().unwrap_or_default()
    }

    fn execute_with_first_segment_cancellation(
        fixture: &Fixture,
        sql: &str,
    ) -> (Result<cassie::executor::QueryResult, CassieError>, u64) {
        let session = fixture.cassie.create_session("reader", None);
        let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
        cassie::midge::adapter::set_query_scan_cancellation_after_entries(Some(1));
        let result = fixture.cassie.execute_sql(&session, sql, vec![]);
        cassie::midge::adapter::set_query_scan_cancellation_after_entries(None);
        let reads = fixture
            .cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_reads);
        (result, reads)
    }

    fn assert_resource_rejection_before_segment_reads(
        fixture: &Fixture,
        result: Result<cassie::executor::QueryResult, CassieError>,
        reads: u64,
        before: &serde_json::Value,
        metric_family: &str,
    ) {
        let error = result.expect_err("metadata-heavy query should reject before segment reads");
        assert!(matches!(error, CassieError::ResourceLimit(_)), "{error:?}");
        assert_eq!(reads, 0, "segment scan started before metadata reservation");
        let after = fixture.cassie.metrics();
        assert_eq!(
            metric(&after, metric_family, "scans"),
            metric(before, metric_family, "scans"),
            "failed metadata path published success"
        );
        assert_eq!(metric(&after, "runtime", "running_queries"), 0);
        assert_eq!(metric(&after, "query", "current_accounted_memory_bytes"), 0);
    }

    #[test]
    fn should_reserve_projection_metadata_before_loading_column_segments() {
        let _guard = cassie::midge::adapter::query_scan_control_test_guard();
        // Arrange
        let fixture = fixture();
        let before = fixture.cassie.metrics();

        // Act
        let (result, reads) = execute_with_first_segment_cancellation(
            &fixture,
            &format!("SELECT id, score, label FROM {COLLECTION} WHERE score >= 0 LIMIT 5"),
        );

        // Assert
        assert_resource_rejection_before_segment_reads(
            &fixture,
            result,
            reads,
            &before,
            "column_batches",
        );
    }

    #[test]
    fn should_reserve_aggregate_metadata_before_validating_column_segments() {
        let _guard = cassie::midge::adapter::query_scan_control_test_guard();
        // Arrange
        let fixture = fixture();
        let before = fixture.cassie.metrics();

        // Act
        let (result, reads) = execute_with_first_segment_cancellation(
            &fixture,
            &format!("SELECT COUNT(*), SUM(score), AVG(score) FROM {COLLECTION}"),
        );

        // Assert
        assert_resource_rejection_before_segment_reads(
            &fixture,
            result,
            reads,
            &before,
            "aggregate_acceleration",
        );
    }
}

// Formerly tests/fulltext_filtered_controls.rs.
mod fulltext_filtered_controls {
    use cassie::app::{Cassie, CassieError};
    use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
    use cassie::midge::adapter::{
        query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
    };
    use serde_json::json;
    use uuid::Uuid;

    use super::support_pgwire as wire;

    const COLLECTION: &str = "fulltext_filtered_controls";
    const QUERY: &str = "SELECT id, body, search_score(body, 'alpha') AS score \
    FROM fulltext_filtered_controls \
    WHERE search(body, 'alpha') AND body <> 'never' LIMIT 8";
    const FIXTURE_ROWS: usize = 64;
    const LOW_MEMORY_BYTES: usize = 1_024;
    const NORMAL_MEMORY_BYTES: usize = 4 * 1024 * 1024;

    struct Fixture {
        cassie: Cassie,
        path: String,
    }

    impl Fixture {
        fn new(memory_budget: usize) -> Self {
            std::env::set_var("CASSIE_STORAGE_MODE", "local");
            let path = std::env::temp_dir()
                .join(format!(
                    "cassie-fulltext-filtered-controls-{}",
                    Uuid::new_v4()
                ))
                .to_string_lossy()
                .into_owned();
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.limits.query_memory_budget_bytes = memory_budget;
            config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
            config.limits.parallel_scan_workers = 1;
            let cassie =
                Cassie::new_with_data_dir_and_config(&path, config).expect("configured cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            cassie
                .execute_sql(
                    &session,
                    &format!("CREATE TABLE {COLLECTION} (body TEXT, category TEXT)"),
                    vec![],
                )
                .expect("create filtered fulltext table");
            let rows = (0..FIXTURE_ROWS)
                .map(|index| {
                    (
                        Some(format!("row-{index:04}")),
                        json!({
                            "body": format!(
                                "alpha controlled filtered fulltext row {index:04} {}",
                                "bounded-payload-".repeat(12)
                            ),
                            "category": "included",
                        }),
                    )
                })
                .collect();
            cassie
                .midge
                .put_fresh_documents(COLLECTION, rows)
                .expect("seed filtered fulltext rows");
            Self { cassie, path }
        }

        fn cleanup(self) {
            drop(self.cassie);
            let _ = std::fs::remove_dir_all(self.path);
        }
    }

    fn metric(metrics: &serde_json::Value, family: &str, name: &str) -> u64 {
        metrics[family][name].as_u64().unwrap_or_default()
    }

    fn assert_failed_metrics_unchanged(before: &serde_json::Value, after: &serde_json::Value) {
        for name in [
            "count",
            "candidate_count_total",
            "result_count_total",
            "retrieval_stage_queries_total",
            "posting_reads_total",
            "candidate_row_fetches_total",
            "row_scan_fallback_total",
        ] {
            assert_eq!(
                metric(after, "search", name),
                metric(before, "search", name),
                "filtered fulltext published failed-path metric search.{name}"
            );
        }
        assert_eq!(
            metric(after, "query", "rows_returned_total"),
            metric(before, "query", "rows_returned_total"),
            "filtered fulltext published partial rows"
        );
    }

    fn assert_cleanup(cassie: &Cassie) {
        let metrics = cassie.metrics();
        assert_eq!(metric(&metrics, "runtime", "running_queries"), 0);
        assert_eq!(
            metric(&metrics, "query", "current_accounted_memory_bytes"),
            0
        );
    }

    fn current_thread_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
    }

    fn error_field(fields: &[(char, String)], tag: char) -> Option<&str> {
        fields
            .iter()
            .find(|(field, _)| *field == tag)
            .map(|(_, value)| value.as_str())
    }

    fn wire_sqlstate(memory_budget: usize, cancel_after_reads: Option<usize>) -> String {
        let fixture = Fixture::new(memory_budget);
        let Fixture { cassie, path } = fixture;
        let runtime = current_thread_runtime();
        let sqlstate = runtime.block_on(async {
            let server = wire::spawn_server(cassie).await;
            let socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect pgwire");
            let (mut reader, mut writer) = tokio::io::split(socket);
            wire::complete_startup(&mut reader, &mut writer).await;
            set_query_scan_cancellation_after_entries(cancel_after_reads);
            wire::write_frames(&mut writer, vec![wire::simple_query_frame(QUERY)]).await;
            let frames = wire::read_frames_until_ready(&mut reader).await;
            set_query_scan_cancellation_after_entries(None);
            let error = frames
                .iter()
                .find(|(tag, _)| *tag == b'E')
                .expect("pgwire filtered fulltext error");
            let fields = wire::parse_error_fields(&error.1);
            let sqlstate = error_field(&fields, 'C')
                .expect("SQLSTATE error field")
                .to_string();
            server.stop().await;
            sqlstate
        });
        let _ = std::fs::remove_dir_all(path);
        sqlstate
    }

    #[test]
    fn should_reject_filtered_fulltext_before_retaining_the_exact_source() {
        let _guard = query_scan_control_test_guard();

        // Arrange
        let fixture = Fixture::new(LOW_MEMORY_BYTES);
        let session = fixture.cassie.create_session("reader", None);
        let before = fixture.cassie.metrics();
        let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();

        // Act
        let error = fixture
            .cassie
            .execute_sql(&session, QUERY, vec![])
            .expect_err("low-budget filtered fulltext query should be atomic");
        let after = fixture.cassie.metrics();
        let reads = fixture
            .cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_reads);

        // Assert
        assert!(matches!(error, CassieError::ResourceLimit(_)));
        assert!(
            reads <= 2,
            "low-memory filtered fulltext read bound: {reads}"
        );
        assert_failed_metrics_unchanged(&before, &after);
        assert_cleanup(&fixture.cassie);
        fixture.cleanup();
        assert_eq!(wire_sqlstate(LOW_MEMORY_BYTES, None), "54000");
    }

    #[test]
    fn should_cancel_filtered_fulltext_after_three_exact_source_reads() {
        let _guard = query_scan_control_test_guard();

        // Arrange
        let fixture = Fixture::new(NORMAL_MEMORY_BYTES);
        let session = fixture.cassie.create_session("reader", None);
        let before = fixture.cassie.metrics();
        let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
        set_query_scan_cancellation_after_entries(Some(3));

        // Act
        let error = fixture
            .cassie
            .execute_sql(&session, QUERY, vec![])
            .expect_err("filtered fulltext exact source should cancel deterministically");
        set_query_scan_cancellation_after_entries(None);
        let after = fixture.cassie.metrics();
        let reads = fixture
            .cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_reads);

        // Assert
        assert!(matches!(error, CassieError::QueryCancelled));
        assert_eq!(reads, 3);
        assert_failed_metrics_unchanged(&before, &after);
        assert_cleanup(&fixture.cassie);
        fixture.cleanup();
        assert_eq!(wire_sqlstate(NORMAL_MEMORY_BYTES, Some(3)), "57014");
    }
}

// Formerly tests/scalar_index_resource_controls.rs.
mod scalar_index_resource_controls {
    use cassie::app::{Cassie, CassieError};
    use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
    use cassie::midge::adapter::{
        query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
    };
    use cassie::types::Value;
    use uuid::Uuid;

    fn configured_cassie(label: &str, memory_budget: usize) -> (Cassie, String) {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = std::env::temp_dir()
            .join(format!("cassie-scalar-controls-{label}-{}", Uuid::new_v4()))
            .to_string_lossy()
            .into_owned();
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_memory_budget_bytes = memory_budget;
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        config.limits.parallel_scan_workers = 1;
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, config).expect("configured cassie");
        cassie.startup().expect("startup");
        (cassie, path)
    }

    fn seed_indexed_rows(cassie: &Cassie, session: &cassie::app::CassieSession, table: &str) {
        cassie
            .execute_sql(
                session,
                &format!("CREATE TABLE {table} (score BIGINT, label TEXT)"),
                vec![],
            )
            .expect("create table");
        for score in 0..32_i64 {
            cassie
                .midge
                .put_document(
                    table,
                    Some(format!("row-{score:04}")),
                    serde_json::json!({"score": score, "label": format!("label-{score:04}")}),
                )
                .expect("seed indexed row");
        }
        cassie
            .execute_sql(
                session,
                &format!("CREATE INDEX {table}_score_idx ON {table} USING btree (score)"),
                vec![],
            )
            .expect("create scalar index");
    }

    #[test]
    fn should_apply_limit_while_iterating_scalar_index_entries() {
        // Arrange
        let _hook_guard = query_scan_control_test_guard();
        let (cassie, path) = configured_cassie("bounded", 64 * 1_024);
        let session = cassie.create_session("tester", None);
        seed_indexed_rows(&cassie, &session, "controlled_scalar_limit");
        let before = cassie.midge.query_scan_entries_for_diagnostics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT score FROM controlled_scalar_limit WHERE score >= 0 ORDER BY score LIMIT 2",
                vec![],
            )
            .expect("bounded scalar index read");
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before);

        // Assert
        assert_eq!(
            result.rows,
            vec![vec![Value::Int64(0)], vec![Value::Int64(1)]]
        );
        assert_eq!(visited, 2, "the native index scan must stop at LIMIT 2");
        assert_eq!(
            cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_scalar_index_hit_before_retaining_it_given_low_memory() {
        // Arrange
        let _hook_guard = query_scan_control_test_guard();
        let (cassie, path) = configured_cassie("low-memory", 64);
        let session = cassie.create_session("tester", None);
        seed_indexed_rows(&cassie, &session, "controlled_scalar_memory");

        // Act
        let error = cassie
        .execute_sql(
            &session,
            "SELECT score FROM controlled_scalar_memory WHERE score >= 0 ORDER BY score LIMIT 1",
            vec![],
        )
        .expect_err("one retained scalar hit should exceed the query budget");

        // Assert
        assert!(matches!(error, CassieError::ResourceLimit(_)));
        assert_eq!(
            cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_cancel_at_a_deterministic_scalar_index_entry_without_leaking_reservations() {
        // Arrange
        let _hook_guard = query_scan_control_test_guard();
        let (cassie, path) = configured_cassie("cancellation", 64 * 1_024);
        let session = cassie.create_session("tester", None);
        seed_indexed_rows(&cassie, &session, "controlled_scalar_cancel");
        let before = cassie.midge.query_scan_entries_for_diagnostics();
        set_query_scan_cancellation_after_entries(Some(3));

        // Act
        let error = cassie
        .execute_sql(
            &session,
            "SELECT score FROM controlled_scalar_cancel WHERE score >= 0 ORDER BY score LIMIT 16",
            vec![],
        )
        .expect_err("the scalar index hook should cancel the query");
        set_query_scan_cancellation_after_entries(None);
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before);

        // Assert
        assert!(matches!(error, CassieError::QueryCancelled));
        assert_eq!(visited, 3);
        assert_eq!(
            cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/vector_ann_concurrency.rs.
mod vector_ann_concurrency {
    use std::sync::{Arc, Barrier};

    use cassie::app::Cassie;
    use cassie::config::{CassieRuntimeConfig, EmbeddingsRuntimeConfig, LocalRuntimeConfig};
    use parking_lot::RwLock;

    use super::support_sql as support;

    const TABLE: &str = "ann_concurrent_source";
    const QUERY: &str = "SELECT id, vector_distance(embedding, '[0,0,0]') AS distance FROM ann_concurrent_source ORDER BY distance ASC LIMIT 5";
    static ANN_RERANK_BARRIER_TEST_GUARD: RwLock<()> = RwLock::new(());

    fn fixture(path: &str, index_type: &str) -> Arc<Cassie> {
        fixture_with_memory_budget(path, index_type, None)
    }

    fn fixture_with_memory_budget(
        path: &str,
        index_type: &str,
        query_memory_budget_bytes: Option<usize>,
    ) -> Arc<Cassie> {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        if let Some(query_memory_budget_bytes) = query_memory_budget_bytes {
            config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
        }
        config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
            model: "deterministic-test".to_string(),
            dimensions: 3,
        });
        let cassie = Arc::new(
            Cassie::new_with_data_dir_and_config(path, config)
                .expect("create concurrent ANN fixture"),
        );
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

    #[test]
    fn should_discard_hnsw_attempt_when_source_changes_before_reranking() {
        // Arrange
        let _ann_rerank_guard = ANN_RERANK_BARRIER_TEST_GUARD.write();
        let _hook_guard = cassie::midge::adapter::query_scan_control_test_guard();
        support::use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = support::data_dir("ann-concurrent-source");
        let cassie = fixture(&path, "hnsw");
        let before = cassie.metrics();
        let selected = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        cassie::executor::set_vector_ann_rerank_barriers(
            Some(Arc::clone(&selected)),
            Some(Arc::clone(&resume)),
        );
        let query_cassie = Arc::clone(&cassie);
        let query = std::thread::spawn(move || {
            query_cassie
                .execute_sql(&query_cassie.create_session("reader", None), QUERY, vec![])
                .expect("concurrent ANN query")
        });
        selected.wait();

        // Act
        cassie
            .execute_sql(
                &cassie.create_session("writer", None),
                "DELETE FROM ann_concurrent_source WHERE id = 'row-0000'",
                vec![],
            )
            .expect("delete selected source row");
        resume.wait();
        let resolved = query.join().expect("query thread");
        cassie
            .execute_sql(
                &cassie.create_session("tester", None),
                "DROP INDEX ann_concurrent_vector ON ann_concurrent_source",
                vec![],
            )
            .expect("drop ANN index for exact baseline");
        let exact = cassie
            .execute_sql(&cassie.create_session("tester", None), QUERY, vec![])
            .expect("exact baseline");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(resolved.rows, exact.rows);
        assert_eq!(
            metrics["vector"]["last_fallback_reason"].as_str(),
            Some("concurrent-source-change")
        );
        assert_eq!(metrics["vector"]["hnsw_executions"].as_u64(), Some(0));
        assert_eq!(
            metrics["vector"]["ann_reads_total"],
            before["vector"]["ann_reads_total"]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_discard_ivfflat_attempt_when_source_is_replaced_before_reranking() {
        // Arrange
        let _ann_rerank_guard = ANN_RERANK_BARRIER_TEST_GUARD.write();
        let _hook_guard = cassie::midge::adapter::query_scan_control_test_guard();
        support::use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = support::data_dir("ivfflat-concurrent-source");
        let cassie = fixture(&path, "ivfflat");
        let before = cassie.metrics();
        let selected = Arc::new(Barrier::new(2));
        let resume = Arc::new(Barrier::new(2));
        cassie::executor::set_vector_ann_rerank_barriers(
            Some(Arc::clone(&selected)),
            Some(Arc::clone(&resume)),
        );
        let query_cassie = Arc::clone(&cassie);
        let query = std::thread::spawn(move || {
            query_cassie
                .execute_sql(&query_cassie.create_session("reader", None), QUERY, vec![])
                .expect("concurrent IVFFlat query")
        });
        selected.wait();

        // Act
        cassie
            .execute_sql(
                &cassie.create_session("writer", None),
                "UPDATE ann_concurrent_source SET embedding = '[9,9,9]' WHERE id = 'row-0000'",
                vec![],
            )
            .expect("replace selected source vector");
        resume.wait();
        let resolved = query.join().expect("query thread");
        cassie
            .execute_sql(
                &cassie.create_session("tester", None),
                "DROP INDEX ann_concurrent_vector ON ann_concurrent_source",
                vec![],
            )
            .expect("drop ANN index for exact baseline");
        let exact = cassie
            .execute_sql(&cassie.create_session("tester", None), QUERY, vec![])
            .expect("exact baseline");
        let metrics = cassie.metrics();

        // Assert
        assert_eq!(resolved.rows, exact.rows);
        assert_eq!(
            metrics["vector"]["last_fallback_reason"].as_str(),
            Some("concurrent-source-change")
        );
        assert_eq!(metrics["vector"]["ivfflat_executions"].as_u64(), Some(0));
        assert_eq!(
            metrics["vector"]["ann_reads_total"],
            before["vector"]["ann_reads_total"]
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_cancel_hnsw_candidate_loading_without_publishing_success_metrics() {
        // Arrange
        let _ann_rerank_guard = ANN_RERANK_BARRIER_TEST_GUARD.read();
        let _guard = cassie::midge::adapter::query_scan_control_test_guard();
        support::use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = support::data_dir("hnsw-controlled-cancellation");
        let cassie = fixture(&path, "hnsw");
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
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_cancel_ivfflat_membership_loading_without_publishing_success_metrics() {
        // Arrange
        let _ann_rerank_guard = ANN_RERANK_BARRIER_TEST_GUARD.read();
        let _guard = cassie::midge::adapter::query_scan_control_test_guard();
        support::use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = support::data_dir("ivfflat-controlled-cancellation");
        let cassie = fixture(&path, "ivfflat");
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
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_hnsw_loading_when_query_memory_is_exhausted() {
        // Arrange
        let _ann_rerank_guard = ANN_RERANK_BARRIER_TEST_GUARD.read();
        let _hook_guard = cassie::midge::adapter::query_scan_control_test_guard();
        support::use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = support::data_dir("hnsw-low-memory");
        let cassie = fixture_with_memory_budget(&path, "hnsw", Some(1_024));
        let before = cassie.metrics();

        // Act
        let error = cassie
            .execute_sql(&cassie.create_session("reader", None), QUERY, vec![])
            .expect_err("accounted HNSW state should exceed the low query budget");
        let after = cassie.metrics();

        // Assert
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
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_publish_hnsw_metrics_only_after_selecting_the_ann_path() {
        // Arrange
        let _ann_rerank_guard = ANN_RERANK_BARRIER_TEST_GUARD.read();
        let _hook_guard = cassie::midge::adapter::query_scan_control_test_guard();
        support::use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = support::data_dir("hnsw-final-metrics");
        let cassie = fixture(&path, "hnsw");
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(&cassie.create_session("reader", None), QUERY, vec![])
            .expect("HNSW query");
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 5);
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
        assert_eq!(
            after["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_ivfflat_loading_when_query_memory_is_exhausted() {
        // Arrange
        let _ann_rerank_guard = ANN_RERANK_BARRIER_TEST_GUARD.read();
        let _hook_guard = cassie::midge::adapter::query_scan_control_test_guard();
        support::use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = support::data_dir("ivfflat-low-memory");
        let cassie = fixture_with_memory_budget(&path, "ivfflat", Some(1_024));
        let before = cassie.metrics();

        // Act
        let error = cassie
            .execute_sql(&cassie.create_session("reader", None), QUERY, vec![])
            .expect_err("accounted IVFFlat state should exceed the low query budget");
        let after = cassie.metrics();

        // Assert
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
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_publish_ivfflat_metrics_only_after_selecting_the_ann_path() {
        // Arrange
        let _ann_rerank_guard = ANN_RERANK_BARRIER_TEST_GUARD.read();
        let _hook_guard = cassie::midge::adapter::query_scan_control_test_guard();
        support::use_local_storage();
        std::env::set_var("CASSIE_EXECUTION_RESULT_CACHE_ENABLED", "false");
        let path = support::data_dir("ivfflat-final-metrics");
        let cassie = fixture(&path, "ivfflat");
        let before = cassie.metrics();

        // Act
        let result = cassie
            .execute_sql(&cassie.create_session("reader", None), QUERY, vec![])
            .expect("IVFFlat query");
        let after = cassie.metrics();

        // Assert
        assert_eq!(result.rows.len(), 5);
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
        assert_eq!(
            after["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/rest_request_cancellation.rs.
mod rest_request_cancellation {
    use cassie::app::{Cassie, CassieError};
    use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
    use cassie::midge::adapter::{
        query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
    };
    use cassie::runtime::QueryCancellationHandle;
    use uuid::Uuid;

    fn configured_cassie(label: &str) -> (Cassie, String) {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = std::env::temp_dir()
            .join(format!(
                "cassie-rest-cancellation-{label}-{}",
                Uuid::new_v4()
            ))
            .to_string_lossy()
            .into_owned();
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.password = "postgres".to_string();
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        config.limits.parallel_scan_workers = 1;
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
        cassie.startup().expect("startup");
        (cassie, path)
    }

    fn seed_rows(cassie: &Cassie, table: &str, count: usize) {
        let rows = (0..count)
            .map(|index| {
                (
                    Some(format!("doc-{index:04}")),
                    serde_json::json!({"payload": format!("value-{index:04}")}),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_documents(table, rows)
            .expect("seed rows");
    }

    #[test]
    fn should_propagate_acknowledged_rest_read_cancellation_without_leaking_resources() {
        // Arrange
        let _guard = query_scan_control_test_guard();
        let (cassie, path) = configured_cassie("read");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_cancelled_read (payload TEXT)",
                vec![],
            )
            .expect("create table");
        seed_rows(&cassie, "rest_cancelled_read", 16);
        set_query_scan_cancellation_after_entries(Some(3));
        let body = br#"{"sql":"SELECT payload FROM rest_cancelled_read"}"#;

        // Act
        let result = cassie::rest::query::execute_with_session_and_cancellation(
            &cassie,
            &session,
            body,
            &QueryCancellationHandle::new(),
        );
        let Err(error) = result else {
            panic!("controlled read must acknowledge cancellation");
        };
        let metrics = cassie.metrics();

        // Assert
        assert!(matches!(error, CassieError::QueryCancelled));
        assert_eq!(metrics["runtime"]["running_queries"].as_u64(), Some(0));
        assert_eq!(
            metrics["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_leave_zero_writes_when_rest_mutation_is_cancelled_before_commit() {
        // Arrange
        let _guard = query_scan_control_test_guard();
        let (cassie, path) = configured_cassie("mutation");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_cancelled_mutation (value BIGINT UNIQUE)",
                vec![],
            )
            .expect("create table");
        let cancellation = QueryCancellationHandle::new();
        cancellation.cancel();
        let body = br#"{"sql":"INSERT INTO rest_cancelled_mutation (value) VALUES (1), (2), (3)"}"#;

        // Act
        let result = cassie::rest::query::execute_with_session_and_cancellation(
            &cassie,
            &session,
            body,
            &cancellation,
        );
        let Err(error) = result else {
            panic!("cancelled mutation must not publish");
        };
        let result = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(*) AS count FROM rest_cancelled_mutation",
                vec![],
            )
            .expect("count rows");

        // Assert
        assert!(matches!(error, CassieError::QueryCancelled));
        assert_eq!(result.rows, vec![vec![cassie::types::Value::Int64(0)]]);
        assert_eq!(
            cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_leave_zero_documents_when_rest_document_write_is_cancelled_before_publication() {
        // Arrange
        let _guard = query_scan_control_test_guard();
        let (cassie, path) = configured_cassie("document-mutation");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE rest_cancelled_document (payload TEXT)",
                vec![],
            )
            .expect("create table");
        let cancellation = QueryCancellationHandle::new();
        cancellation.cancel();

        // Act
        let result = cassie::rest::documents::create_with_cancellation(
            &cassie,
            "rest_cancelled_document",
            br#"{"payload":"must-not-commit"}"#,
            &cancellation,
        );
        let Err(error) = result else {
            panic!("cancelled REST document write must not publish");
        };
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT COUNT(*) AS count FROM rest_cancelled_document",
                vec![],
            )
            .expect("count documents");

        // Assert
        assert!(matches!(error, CassieError::QueryCancelled));
        assert_eq!(rows.rows, vec![vec![cassie::types::Value::Int64(0)]]);

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/graph_resilience.rs.
mod graph_resilience {
    use cassie::app::{Cassie, CassieError, CassieSession};
    use cassie::catalog::GraphMeta;
    use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
    use cassie::midge::adapter::{set_query_scan_cancellation_after_entries, StorageFamily};
    use cassie::types::Value;
    use serde_json::json;

    use super::support_graph_evidence::SeededGraphFixture;
    use super::support_sql as support;
    use support::{data_dir, use_local_storage};

    fn current_thread_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
    }

    fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) {
        cassie
            .execute_sql(session, sql, vec![])
            .expect("execute graph statement");
    }

    #[test]
    fn should_preserve_seeded_graph_results_across_path_permutations() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        let forward = SeededGraphFixture::from_seed(0x00CA_551E, false).compare();
        let reverse = SeededGraphFixture::from_seed(0x00CA_551E, true).compare();
        let other_seed = SeededGraphFixture::from_seed(0x000A_11CE, false).compare();

        // Act
        let forward_rows = forward.adjacency.clone();
        let reverse_rows = reverse.adjacency.clone();

        // Assert
        assert_eq!(forward.native_overlay, forward_rows);
        assert_eq!(reverse.native_overlay, reverse_rows);
        assert_eq!(forward_rows, reverse_rows);
        assert_ne!(forward_rows, other_seed.adjacency);
        assert!(forward.disconnected_native_overlay.is_empty());
        assert!(forward.disconnected_adjacency.is_empty());
        assert_eq!(forward.overlay_fallback_reason, "transaction-overlay");
    }

    fn configured_cassie(path: &str, memory_budget: usize) -> Cassie {
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_memory_budget_bytes = memory_budget;
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        config.limits.parallel_scan_workers = 1;
        let cassie = Cassie::new_with_data_dir_and_config(path, config).expect("configured cassie");
        cassie.startup().expect("startup");
        cassie
    }

    fn graph_edge_payload(
        edge_id: &str,
        source_id: &str,
        target_id: &str,
        edge_type: &str,
        weight: f64,
    ) -> serde_json::Value {
        json!({
            "edge_id": edge_id,
            "source_type": "person",
            "source_id": source_id,
            "target_type": "person",
            "target_id": target_id,
            "edge_type": edge_type,
            "weight": weight,
        })
    }

    #[test]
    fn should_keep_colon_containing_node_identities_distinct_in_shortest_path() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_colon_identity");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        execute(
            &cassie,
            &session,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('dead', 'root', 'start', 'a:b', 'c', 'knows', 1), ('route', 'root', 'start', 'a', 'b:c', 'knows', 2), ('finish', 'a', 'b:c', 'goal', 'finish', 'knows', 1)",
        );

        // Act
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT node_type, node_id, cost, depth FROM graph_shortest_path('social', 'root', 'start', 'goal', 'finish', 3, 'out', 'knows', 1)",
                vec![],
            )
            .expect("shortest path")
            .rows;

        // Assert
        assert_eq!(
            rows,
            vec![vec![
                Value::String("goal".into()),
                Value::String("finish".into()),
                Value::Float64(3.0),
                Value::Int64(2),
            ]]
        );
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_merge_both_directions_by_weight_then_edge_id_before_limit() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_weighted_limit");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        execute(
            &cassie,
            &session,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e3', 'person', 'alice', 'person', 'dana', 'knows', 1), ('e1', 'person', 'bob', 'person', 'alice', 'knows', 1), ('e2', 'person', 'alice', 'person', 'carol', 'knows', 1), ('e0', 'person', 'erin', 'person', 'alice', 'knows', 2), ('e4', 'person', 'alice', 'person', 'frank', 'knows', 0.5)",
        );

        // Act
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT edge_id, cost, node_id FROM graph_neighbors('social', 'person', 'alice', 'both', 'knows', 4)",
                vec![],
            )
            .expect("weighted neighbors")
            .rows;

        // Assert
        assert_eq!(
            rows,
            vec![
                vec![
                    Value::String("e4".into()),
                    Value::Float64(0.5),
                    Value::String("frank".into()),
                ],
                vec![
                    Value::String("e1".into()),
                    Value::Float64(1.0),
                    Value::String("bob".into()),
                ],
                vec![
                    Value::String("e2".into()),
                    Value::Float64(1.0),
                    Value::String("carol".into()),
                ],
                vec![
                    Value::String("e3".into()),
                    Value::Float64(1.0),
                    Value::String("dana".into()),
                ],
            ]
        );
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_return_a_self_loop_once_when_scanning_both_directions() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_self_loop_both");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        execute(
            &cassie,
            &session,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('loop', 'person', 'alice', 'person', 'alice', 'knows', 1)",
        );

        // Act
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT edge_id, node_id FROM graph_neighbors('social', 'person', 'alice', 'both', 'knows', 10)",
                vec![],
            )
            .expect("self-loop neighbors")
            .rows;

        // Assert
        assert_eq!(
            rows,
            vec![vec![
                Value::String("loop".into()),
                Value::String("alice".into()),
            ]]
        );
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_fail_graph_traversal_atomically_given_low_query_memory() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_low_memory");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = configured_cassie(&path, 64);
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        let edges = (0..8)
            .map(|index| {
                let edge_id = format!("edge-{index:02}");
                let target_id = format!("neighbor-with-a-retained-identity-{index:02}");
                (
                    Some(edge_id.clone()),
                    graph_edge_payload(&edge_id, "alice", &target_id, "knows", index.into()),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_graph_documents("social_edges", edges)
            .expect("seed graph edges");
        let before = cassie.metrics();

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT node_id FROM graph_expand('social', 'person', 'alice', 2, 'out', 'knows', 8)",
                vec![],
            )
            .expect_err("retained traversal state should exceed the query budget");
        let after = cassie.metrics();

        // Assert
        assert!(
            matches!(error, CassieError::ResourceLimit(_)),
            "expected SQLSTATE 54000 resource limit, got {error:?}"
        );
        assert_eq!(
            after["graph"]["traversals"], before["graph"]["traversals"],
            "a failed traversal must not publish success metrics"
        );
        assert_eq!(
            after["graph"]["rows"], before["graph"]["rows"],
            "a failed traversal must not publish partial rows"
        );
        assert_eq!(
            after["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_cancel_graph_scan_at_a_deterministic_entry_without_partial_metrics() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_deterministic_cancellation");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = configured_cassie(&path, 64 * 1_024);
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        let edges = (0..8)
            .map(|index| {
                let edge_id = format!("edge-{index:02}");
                let target_id = format!("neighbor-{index:02}");
                (
                    Some(edge_id.clone()),
                    graph_edge_payload(&edge_id, "alice", &target_id, "knows", index.into()),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_graph_documents("social_edges", edges)
            .expect("seed graph edges");
        let before_metrics = cassie.metrics();
        let before_entries = cassie.midge.query_scan_entries_for_diagnostics();
        set_query_scan_cancellation_after_entries(Some(3));

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT node_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 8)",
                vec![],
            )
            .expect_err("the controlled graph scan should observe cancellation");
        set_query_scan_cancellation_after_entries(None);
        let after_metrics = cassie.metrics();
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_entries);

        // Assert
        assert!(
            matches!(error, CassieError::QueryCancelled),
            "expected SQLSTATE 57014 cancellation, got {error:?}"
        );
        assert_eq!(visited, 3);
        assert_eq!(
            after_metrics["graph"]["traversals"],
            before_metrics["graph"]["traversals"]
        );
        assert_eq!(
            after_metrics["graph"]["rows"],
            before_metrics["graph"]["rows"]
        );
        assert_eq!(
            after_metrics["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_match_native_results_after_transaction_overlay_commit() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_overlay_fallback");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("writer", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        execute(&cassie, &session, "BEGIN");
        execute(
            &cassie,
            &session,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e2', 'person', 'alice', 'person', 'carol', 'knows', 2), ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1)",
        );

        // Act
        let overlay = cassie
            .execute_sql(
                &session,
                "SELECT edge_id, node_id, cost FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("transaction overlay traversal")
            .rows;
        let overlay_metrics = cassie.metrics();
        execute(&cassie, &session, "COMMIT");
        let native = cassie
            .execute_sql(
                &session,
                "SELECT edge_id, node_id, cost FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("native traversal")
            .rows;

        // Assert
        assert_eq!(overlay, native);
        assert_eq!(
            overlay_metrics["graph"]["last_fallback_reason"],
            "transaction-overlay"
        );
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_preserve_graph_adjacency_across_schema_rename() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_schema_rename_drop");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", Some("postgres".to_string()));
        execute(&cassie, &session, "CREATE SCHEMA reporting");
        execute(
            &cassie,
            &session,
            "CREATE TABLE reporting.social_nodes (node_type TEXT, node_id TEXT)",
        );
        execute(
            &cassie,
            &session,
            "CREATE TABLE reporting.social_edges (edge_id TEXT, source_type TEXT, source_id TEXT, target_type TEXT, target_id TEXT, edge_type TEXT, weight FLOAT)",
        );
        let graph = GraphMeta::new("postgres.reporting.social");
        cassie.midge.put_graph(&graph).expect("persist graph metadata");
        cassie.catalog.register_graph(graph);
        execute(
            &cassie,
            &session,
            "INSERT INTO reporting.social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1)",
        );
        let storage_id_before_rename = cassie
            .midge
            .list_graphs()
            .expect("graphs before rename")[0]
            .storage_id;

        // Act
        execute(
            &cassie,
            &session,
            "ALTER SCHEMA reporting RENAME TO reporting_archive",
        );
        let renamed = cassie
            .execute_sql(
                &session,
                "SELECT edge_id FROM graph_neighbors('reporting_archive.social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("renamed graph traversal")
            .rows;
        let old_name = cassie.execute_sql(
            &session,
            "SELECT edge_id FROM graph_neighbors('reporting.social', 'person', 'alice', 'out', 'knows', 10)",
            vec![],
        );
        let storage_id_after_rename = cassie
            .midge
            .list_graphs()
            .expect("graphs after rename")[0]
            .storage_id;

        // Assert
        assert_eq!(renamed, vec![vec![Value::String("e1".into())]]);
        assert!(old_name.is_err());
        assert_eq!(storage_id_after_rename, storage_id_before_rename);
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_remove_graph_adjacency_after_edge_collection_drop() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_edge_collection_drop");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        execute(
            &cassie,
            &session,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1)",
        );
        let before_drop = cassie
            .execute_sql(
                &session,
                "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("graph traversal before edge collection drop")
            .rows;

        // Act
        execute(&cassie, &session, "DROP TABLE social_edges");
        let after_drop = cassie
            .execute_sql(
                &session,
                "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("graph traversal after edge collection drop")
            .rows;
        drop(cassie);
        let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
        restarted.startup().expect("startup after edge collection drop");
        let restarted_session = restarted.create_session("tester", None);
        let after_restart = restarted
            .execute_sql(
                &restarted_session,
                "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("graph traversal after dropped collection restart")
            .rows;

        // Assert
        assert_eq!(before_drop, vec![vec![Value::String("e1".into())]]);
        assert!(after_drop.is_empty());
        assert!(after_restart.is_empty());
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_retain_graph_traversal_order_after_restart() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_restart");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let before_restart = {
            let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            execute(&cassie, &session, "CREATE GRAPH social");
            execute(
                &cassie,
                &session,
                "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e2', 'person', 'alice', 'person', 'carol', 'knows', 2), ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1)",
            );
            cassie
                .execute_sql(
                    &session,
                    "SELECT edge_id, node_id, cost FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                    vec![],
                )
                .expect("traversal before restart")
                .rows
        };

        // Act
        let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
        restarted.startup().expect("restart startup");
        let session = restarted.create_session("tester", None);
        let after_restart = restarted
            .execute_sql(
                &session,
                "SELECT edge_id, node_id, cost FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 10)",
                vec![],
            )
            .expect("traversal after restart")
            .rows;

        // Assert
        assert_eq!(after_restart, before_restart);
        let _ = std::fs::remove_dir_all(path);
    });
    }

    #[test]
    fn should_rebuild_inconsistent_graph_adjacency_during_startup() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_rebuild_inconsistent_sidecar");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir(&path).expect("cassie");
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        execute(
            &cassie,
            &session,
            "INSERT INTO social_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('e1', 'person', 'alice', 'person', 'bob', 'knows', 1)",
        );
        let entries = cassie
            .midge
            .raw_scan_prefix(StorageFamily::Data, b"")
            .expect("data entries");
        let manifest_key = entries
            .iter()
            .find(|(_, value)| {
                serde_json::from_slice::<serde_json::Value>(value).is_ok_and(|value| {
                    value.get("format_version").is_some()
                        && value.get("source_generation").is_some()
                        && value.get("edge_count").is_some()
                })
            })
            .map(|(key, _)| key)
            .expect("graph manifest");
        let adjacency_key = entries
            .iter()
            .filter(|(_, value)| value.is_empty())
            .max_by_key(|(key, _)| common_prefix_len(key, manifest_key))
            .map(|(key, _)| key)
            .expect("graph adjacency entry");
        cassie
            .midge
            .raw_delete(StorageFamily::Data, adjacency_key)
            .expect("remove one adjacency entry");
        drop(cassie);

        // Act
        let restarted = Cassie::new_with_data_dir(&path).expect("restarted cassie");
        restarted.startup().expect("startup rebuild");
        let restarted_session = restarted.create_session("tester", None);
        let before_entries = restarted.midge.query_scan_entries_for_diagnostics();
        let rows = restarted
            .execute_sql(
                &restarted_session,
                "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 1)",
                vec![],
            )
            .expect("rebuilt native traversal")
            .rows;
        let visited = restarted
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_entries);

        // Assert
        assert_eq!(rows, vec![vec![Value::String("e1".into())]]);
        assert_eq!(visited, 1, "startup should rebuild the bounded sidecar");
        let _ = std::fs::remove_dir_all(path);
    });
    }

    fn common_prefix_len(left: &[u8], right: &[u8]) -> usize {
        left.iter()
            .zip(right)
            .take_while(|(left, right)| left == right)
            .count()
    }

    #[test]
    fn should_bound_filtered_native_graph_reads_to_the_requested_edge_type() {
        // Arrange
        let _suite_query_scan_guard = cassie::midge::adapter::query_scan_control_test_guard();
        use_local_storage();
        let path = data_dir("graph_bounded_native_reads");
        let runtime = current_thread_runtime();
        runtime.block_on(async {
        let cassie = configured_cassie(&path, 64 * 1_024);
        let session = cassie.create_session("tester", None);
        execute(&cassie, &session, "CREATE GRAPH social");
        let mut edges = (0..64)
            .map(|index| {
                let edge_id = format!("noise-{index:02}");
                let target_id = format!("noise-node-{index:02}");
                (
                    Some(edge_id.clone()),
                    graph_edge_payload(&edge_id, "alice", &target_id, "ignored", index.into()),
                )
            })
            .collect::<Vec<_>>();
        edges.extend([
            (
                Some("knows-2".to_string()),
                graph_edge_payload("knows-2", "alice", "carol", "knows", 2.0),
            ),
            (
                Some("knows-1".to_string()),
                graph_edge_payload("knows-1", "alice", "bob", "knows", 1.0),
            ),
        ]);
        cassie
            .midge
            .put_fresh_graph_documents("social_edges", edges)
            .expect("seed graph edges");
        let before_entries = cassie.midge.query_scan_entries_for_diagnostics();
        let before_reads = cassie.metrics()["storage"]["data"]["reads"]
            .as_u64()
            .unwrap_or_default();

        // Act
        let rows = cassie
            .execute_sql(
                &session,
                "SELECT edge_id FROM graph_neighbors('social', 'person', 'alice', 'out', 'knows', 1)",
                vec![],
            )
            .expect("bounded filtered graph scan")
            .rows;
        let after = cassie.metrics();
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_entries);
        let reads = after["storage"]["data"]["reads"]
            .as_u64()
            .unwrap_or_default()
            .saturating_sub(before_reads);

        // Assert
        assert_eq!(rows, vec![vec![Value::String("knows-1".into())]]);
        assert!(visited > 0, "native graph reads must be observable");
        assert!(visited <= 2, "expected bounded edge-type reads, got {visited}");
        assert!(reads <= 4, "expected bounded storage reads, got {reads}");
        assert_eq!(
            after["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        let _ = std::fs::remove_dir_all(path);
    });
    }
}

// Formerly tests/query_resource_controls.rs.
mod query_resource_controls {
    use cassie::app::{Cassie, CassieError};
    use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
    use cassie::executor::projection::set_projection_build_failure_point;
    use cassie::midge::adapter::{
        query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
    };
    use cassie::types::Value;
    use uuid::Uuid;

    use super::support_pgwire as wire;

    fn data_dir(label: &str) -> String {
        crate::support_temp_dirs::sweep_stale_once();
        std::env::temp_dir()
            .join(format!("cassie-query-controls-{label}-{}", Uuid::new_v4()))
            .to_string_lossy()
            .into_owned()
    }

    fn configured_cassie(label: &str, memory_budget: usize) -> (Cassie, String) {
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = data_dir(label);
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_memory_budget_bytes = memory_budget;
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        config.limits.parallel_scan_workers = 1;
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, config).expect("configured cassie");
        cassie.startup().expect("startup");
        (cassie, path)
    }

    fn seed_documents(cassie: &Cassie, table: &str, count: usize, payload_size: usize) {
        let rows = (0..count)
            .map(|index| {
                (
                    Some(format!("doc-{index:04}")),
                    serde_json::json!({
                        "payload": format!("{index:04}-{}", "x".repeat(payload_size)),
                    }),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_documents(table, rows)
            .expect("seed documents");
    }

    fn error_field(fields: &[(char, String)], tag: char) -> Option<&str> {
        fields
            .iter()
            .find(|(field, _)| *field == tag)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn should_reject_unbounded_scan_without_partial_rows_given_low_memory_budget() {
        // Arrange
        let _hook_guard = query_scan_control_test_guard();
        let (cassie, path) = configured_cassie("low-scan-budget", 512);
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_scan_budget (payload TEXT)",
                vec![],
            )
            .expect("create table");
        seed_documents(&cassie, "controlled_scan_budget", 32, 256);

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT payload FROM controlled_scan_budget",
                vec![],
            )
            .expect_err("unbounded scan should exceed the query budget");
        let metrics = cassie.metrics();

        // Assert
        assert!(matches!(error, CassieError::ResourceLimit(_)));
        assert!(error.to_string().contains("query memory budget exceeded"));
        assert_eq!(metrics["runtime"]["running_queries"].as_u64(), Some(0));
        assert_eq!(
            metrics["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        assert_eq!(
            metrics["query"]["errors_by_class"]["resource_limit"].as_u64(),
            Some(1)
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_stop_limit_scan_before_low_memory_budget_is_exhausted() {
        // Arrange
        let _hook_guard = query_scan_control_test_guard();
        let (cassie, path) = configured_cassie("limit-early-stop", 512);
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_limit_scan (payload TEXT)",
                vec![],
            )
            .expect("create table");
        seed_documents(&cassie, "controlled_limit_scan", 64, 1_024);
        let before = cassie.midge.query_scan_entries_for_diagnostics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM controlled_limit_scan LIMIT 1",
                vec![],
            )
            .expect("LIMIT should avoid retaining the complete scan");
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before);

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(visited, 1, "LIMIT 1 must consume one native row");
        assert_eq!(
            cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_stop_exists_scan_after_first_inner_row_given_low_memory_budget() {
        // Arrange
        let _hook_guard = query_scan_control_test_guard();
        let (cassie, path) = configured_cassie("exists-early-stop", 768);
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_exists_outer (payload TEXT)",
                vec![],
            )
            .expect("create outer table");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_exists_inner (payload TEXT)",
                vec![],
            )
            .expect("create inner table");
        seed_documents(&cassie, "controlled_exists_outer", 1, 16);
        seed_documents(&cassie, "controlled_exists_inner", 64, 1_024);
        let before = cassie.midge.query_scan_entries_for_diagnostics();

        // Act
        let result = cassie
        .execute_sql(
            &session,
            "SELECT id FROM controlled_exists_outer WHERE EXISTS (SELECT id FROM controlled_exists_inner)",
            vec![],
        )
        .expect("EXISTS should stop after the first inner row");
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before);

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            visited, 2,
            "outer and inner scans should each consume one row"
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_preserve_transaction_overlay_visibility_under_query_controls() {
        // Arrange
        let _hook_guard = query_scan_control_test_guard();
        let (cassie, path) = configured_cassie("transaction-overlay", 8 * 1_024);
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_overlay_visibility (payload TEXT)",
                vec![],
            )
            .expect("create table");
        seed_documents(&cassie, "controlled_overlay_visibility", 1, 16);
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin transaction");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO controlled_overlay_visibility (payload) VALUES ('staged')",
                vec![],
            )
            .expect("stage insert");

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT payload FROM controlled_overlay_visibility ORDER BY payload",
                vec![],
            )
            .expect("overlay query");

        // Assert
        assert_eq!(result.rows.len(), 2);
        assert!(result
            .rows
            .contains(&vec![Value::String("staged".to_string())]));
        cassie
            .execute_sql(&session, "ROLLBACK", vec![])
            .expect("rollback transaction");

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_bound_native_reads_for_limit_with_transaction_overlay() {
        // Arrange
        let _hook_guard = query_scan_control_test_guard();
        let (cassie, path) = configured_cassie("overlay-limit", 16 * 1_024 * 1_024);
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_overlay_limit (payload TEXT)",
                vec![],
            )
            .expect("create table");
        seed_documents(&cassie, "controlled_overlay_limit", 64, 64);
        cassie
            .execute_sql(&session, "BEGIN", vec![])
            .expect("begin transaction");
        cassie
            .execute_sql(
                &session,
                "INSERT INTO controlled_overlay_limit (payload) VALUES ('staged')",
                vec![],
            )
            .expect("stage insert");
        let before = cassie.midge.query_scan_entries_for_diagnostics();

        // Act
        let result = cassie
            .execute_sql(
                &session,
                "SELECT id FROM controlled_overlay_limit LIMIT 1",
                vec![],
            )
            .expect("bounded overlay query");
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before);

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert_eq!(
            visited, 1,
            "overlay LIMIT must not clone the persisted collection"
        );
        cassie
            .execute_sql(&session, "ROLLBACK", vec![])
            .expect("rollback transaction");

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_report_join_budget_failure_with_program_limit_sqlstate() {
        // Arrange
        let _hook_guard = query_scan_control_test_guard();
        std::env::set_var("CASSIE_STORAGE_MODE", "local");
        let path = data_dir("join-sqlstate");
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_memory_budget_bytes = 4 * 1_024;
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");

        runtime.block_on(async {
        let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("cassie");
        cassie.startup().expect("startup");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_join_left (payload TEXT)",
                vec![],
            )
            .expect("create left table");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_join_right (payload TEXT)",
                vec![],
            )
            .expect("create right table");
        seed_documents(&cassie, "controlled_join_left", 16, 48);
        seed_documents(&cassie, "controlled_join_right", 16, 48);
        let server = wire::spawn_server(cassie).await;
        let socket = tokio::net::TcpStream::connect(server.addr)
            .await
            .expect("connect pgwire");
        let (mut reader, mut writer) = tokio::io::split(socket);
        wire::complete_startup(&mut reader, &mut writer).await;

        // Act
        wire::write_frames(
            &mut writer,
            vec![wire::simple_query_frame(
                "SELECT controlled_join_left.payload, controlled_join_right.payload FROM controlled_join_left CROSS JOIN controlled_join_right",
            )],
        )
        .await;
        let frames = wire::read_frames_until_ready(&mut reader).await;

        // Assert
        let error = frames
            .iter()
            .find(|(tag, _)| *tag == b'E')
            .expect("join resource error");
        let fields = wire::parse_error_fields(&error.1);
        assert_eq!(error_field(&fields, 'C'), Some("54000"));
        assert!(error_field(&fields, 'M')
            .expect("error message")
            .contains("query memory budget exceeded"));

        server.stop().await;
    });

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_stop_cross_join_after_limit_without_materializing_both_inputs() {
        // Arrange
        let _hook_guard = query_scan_control_test_guard();
        let (cassie, path) = configured_cassie("cross-join-limit", 8 * 1_024);
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_cross_left (payload TEXT)",
                vec![],
            )
            .expect("create left table");
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_cross_right (payload TEXT)",
                vec![],
            )
            .expect("create right table");
        seed_documents(&cassie, "controlled_cross_left", 64, 1_024);
        seed_documents(&cassie, "controlled_cross_right", 64, 1_024);
        let before = cassie.midge.query_scan_entries_for_diagnostics();

        // Act
        let result = cassie
        .execute_sql(
            &session,
            "SELECT controlled_cross_left.payload, controlled_cross_right.payload FROM controlled_cross_left CROSS JOIN controlled_cross_right LIMIT 1",
            vec![],
        )
        .expect("LIMIT should bound both cross-join inputs and output");
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before);

        // Assert
        assert_eq!(result.rows.len(), 1);
        assert!(
            visited <= 2,
            "LIMIT 1 cross join should consume at most one row from each input, visited {visited}"
        );
        assert_eq!(
            cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_cancel_at_a_deterministic_mid_scan_boundary_without_leaking_reservations() {
        // Arrange
        let _hook_guard = query_scan_control_test_guard();
        let (cassie, path) = configured_cassie("deterministic-cancellation", 64 * 1_024);
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_mid_scan_cancel (payload TEXT)",
                vec![],
            )
            .expect("create table");
        seed_documents(&cassie, "controlled_mid_scan_cancel", 64, 128);
        let before = cassie.midge.query_scan_entries_for_diagnostics();
        set_query_scan_cancellation_after_entries(Some(3));

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT payload FROM controlled_mid_scan_cancel",
                vec![],
            )
            .expect_err("deterministic scan hook should cancel the query");
        set_query_scan_cancellation_after_entries(None);
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before);
        let metrics = cassie.metrics();

        // Assert
        assert!(matches!(error, CassieError::QueryCancelled));
        assert_eq!(visited, 3);
        assert_eq!(metrics["runtime"]["running_queries"].as_u64(), Some(0));
        assert_eq!(
            metrics["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );

        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_cancel_unindexed_heap_top_k_at_controlled_scan_boundary() {
        // Arrange
        let _hook_guard = query_scan_control_test_guard();
        let (cassie, path) = configured_cassie("heap-top-k-cancellation", 64 * 1_024);
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_heap_top_k_cancel (payload TEXT)",
                vec![],
            )
            .expect("create table");
        seed_documents(&cassie, "controlled_heap_top_k_cancel", 64, 128);
        let before = cassie.midge.query_scan_entries_for_diagnostics();
        set_query_scan_cancellation_after_entries(Some(3));

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT payload FROM controlled_heap_top_k_cancel ORDER BY payload LIMIT 5",
                vec![],
            )
            .expect_err("heap top-k should observe controlled cancellation");
        set_query_scan_cancellation_after_entries(None);
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before);

        // Assert
        assert!(matches!(error, CassieError::QueryCancelled));
        assert_eq!(visited, 3);
        assert_eq!(
            cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_unindexed_heap_top_k_before_exceeding_memory_budget() {
        // Arrange
        let _hook_guard = query_scan_control_test_guard();
        let (cassie, path) = configured_cassie("heap-top-k-memory", 1_024);
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_heap_top_k_memory (payload TEXT)",
                vec![],
            )
            .expect("create table");
        seed_documents(&cassie, "controlled_heap_top_k_memory", 64, 256);

        // Act
        let error = cassie
            .execute_sql(
                &session,
                "SELECT payload FROM controlled_heap_top_k_memory ORDER BY payload LIMIT 10",
                vec![],
            )
            .expect_err("heap top-k should respect query memory budget");

        // Assert
        assert!(matches!(error, CassieError::ResourceLimit(_)));
        assert!(error.to_string().contains("query memory budget exceeded"));
        assert_eq!(
            cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_reject_expanding_projection_before_building_output() {
        let _hook_guard = query_scan_control_test_guard();
        // Arrange
        let (cassie, path) = configured_cassie("expanding-projection-memory", 1_024);
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_expanding_projection (payload TEXT)",
                vec![],
            )
            .expect("create table");
        seed_documents(&cassie, "controlled_expanding_projection", 1, 256);
        set_projection_build_failure_point(true);

        // Act
        let error = cassie
        .execute_sql(
            &session,
            "SELECT concat(payload, payload, payload, payload) AS expanded FROM controlled_expanding_projection",
            vec![],
        )
        .expect_err("projection should reserve expansion before building");
        set_projection_build_failure_point(false);

        // Assert
        assert!(
            matches!(error, CassieError::ResourceLimit(_)),
            "unexpected expanding projection error: {error:?}"
        );
        assert!(error.to_string().contains("query memory budget exceeded"));
        assert_eq!(
            cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn should_cancel_wildcard_scan_at_the_same_controlled_storage_boundary() {
        // Arrange
        let _hook_guard = query_scan_control_test_guard();
        let (cassie, path) = configured_cassie("wildcard-cancellation", 64 * 1_024);
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                "CREATE TABLE controlled_wildcard_cancel (payload TEXT)",
                vec![],
            )
            .expect("create table");
        seed_documents(&cassie, "controlled_wildcard_cancel", 64, 128);
        let before = cassie.midge.query_scan_entries_for_diagnostics();
        set_query_scan_cancellation_after_entries(Some(4));

        // Act
        let error = cassie
            .execute_sql(&session, "SELECT * FROM controlled_wildcard_cancel", vec![])
            .expect_err("wildcard scan should observe the controlled cursor cancellation");
        let visited = cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before);

        // Assert
        assert!(matches!(error, CassieError::QueryCancelled));
        assert_eq!(visited, 4);
        assert_eq!(
            cassie.metrics()["query"]["current_accounted_memory_bytes"].as_u64(),
            Some(0)
        );

        let _ = std::fs::remove_dir_all(path);
    }
}

// Formerly tests/specialized_query_controls.rs.
mod specialized_query_controls {
    use cassie::app::{Cassie, CassieError, CassieSession};
    use cassie::config::{CassieRuntimeConfig, ExecutionResultCacheEnabled};
    use cassie::midge::adapter::{
        query_scan_control_test_guard, set_query_scan_cancellation_after_entries,
    };
    use serde_json::json;
    use uuid::Uuid;

    use super::support_pgwire as wire;

    const FIXTURE_ROWS: usize = 64;
    const RESULT_LIMIT: usize = 5;
    const LOW_MEMORY_BYTES: usize = 1_024;
    const NORMAL_MEMORY_BUDGET: usize = 1024 * 1024;

    #[derive(Debug, Clone, Copy)]
    enum AnalyticalFamily {
        TimeSeries,
        ColumnProjection,
        ColumnAggregate,
        Graph,
    }

    impl AnalyticalFamily {
        const ALL: [Self; 4] = [
            Self::TimeSeries,
            Self::ColumnProjection,
            Self::ColumnAggregate,
            Self::Graph,
        ];

        const fn label(self) -> &'static str {
            match self {
                Self::TimeSeries => "time-series",
                Self::ColumnProjection => "column-projection",
                Self::ColumnAggregate => "column-aggregate",
                Self::Graph => "graph",
            }
        }

        const fn query(self) -> &'static str {
            match self {
            Self::TimeSeries => {
                "SELECT id, amount FROM controlled_time_series WHERE event_at >= '2026-01-01T00:00:00Z' LIMIT 5"
            }
            Self::ColumnProjection => {
                "SELECT id, score, label FROM controlled_column_projection WHERE score >= 0 LIMIT 5"
            }
            Self::ColumnAggregate => {
                "SELECT COUNT(*), SUM(score), AVG(score) FROM controlled_column_aggregate"
            }
            Self::Graph => {
                "SELECT edge_id, node_id, cost FROM graph_neighbors('controlled_graph', 'person', 'root', 'out', 'knows', 5) LIMIT 5"
            }
        }
        }

        fn successful_paths(self, metrics: &serde_json::Value) -> u64 {
            match self {
                Self::TimeSeries => metrics["time_series"]["scans"].as_u64().unwrap_or_default(),
                Self::ColumnProjection => metrics["column_batches"]["scans"]
                    .as_u64()
                    .unwrap_or_default(),
                Self::ColumnAggregate => metrics["aggregate_acceleration"]["scans"]
                    .as_u64()
                    .unwrap_or_default(),
                Self::Graph => metrics["graph"]["traversals"].as_u64().unwrap_or_default(),
            }
        }

        const fn controlled_read_bound(self) -> u64 {
            match self {
                Self::TimeSeries => (3 * FIXTURE_ROWS + 1) as u64,
                Self::ColumnProjection | Self::ColumnAggregate | Self::Graph => FIXTURE_ROWS as u64,
            }
        }
    }

    struct Fixture {
        cassie: Cassie,
        session: CassieSession,
        path: String,
    }

    struct FallbackEvidence {
        rows: Vec<Vec<cassie::types::Value>>,
        overlay_metrics: Option<serde_json::Value>,
        final_metrics: serde_json::Value,
    }

    impl Fixture {
        fn new(family: AnalyticalFamily, memory_budget: usize) -> Self {
            std::env::set_var("CASSIE_STORAGE_MODE", "local");
            let path = std::env::temp_dir()
                .join(format!(
                    "cassie-specialized-analytical-{}-{}",
                    family.label(),
                    Uuid::new_v4()
                ))
                .to_string_lossy()
                .into_owned();
            let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
            config.limits.query_memory_budget_bytes = memory_budget;
            config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
            config.limits.parallel_scan_workers = 1;
            let cassie =
                Cassie::new_with_data_dir_and_config(&path, config).expect("configured cassie");
            cassie.startup().expect("startup");
            let session = cassie.create_session("tester", None);
            seed_family(&cassie, &session, family);
            Self {
                cassie,
                session,
                path,
            }
        }

        fn cleanup(self) {
            drop(self.cassie);
            let _ = std::fs::remove_dir_all(self.path);
        }
    }

    fn execute(cassie: &Cassie, session: &CassieSession, sql: &str) {
        cassie
            .execute_sql(session, sql, vec![])
            .unwrap_or_else(|error| panic!("execute {sql}: {error}"));
    }

    fn current_thread_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
    }

    fn error_field(fields: &[(char, String)], tag: char) -> Option<&str> {
        fields
            .iter()
            .find(|(field, _)| *field == tag)
            .map(|(_, value)| value.as_str())
    }

    fn wire_sqlstate(
        family: AnalyticalFamily,
        memory_budget: usize,
        cancel_after_reads: Option<usize>,
    ) -> String {
        let fixture = Fixture::new(family, memory_budget);
        let Fixture {
            cassie,
            session,
            path,
        } = fixture;
        drop(session);
        let runtime = current_thread_runtime();
        let sqlstate = runtime.block_on(async {
            let server = wire::spawn_server(cassie).await;
            let socket = tokio::net::TcpStream::connect(server.addr)
                .await
                .expect("connect pgwire");
            let (mut reader, mut writer) = tokio::io::split(socket);
            wire::complete_startup(&mut reader, &mut writer).await;
            set_query_scan_cancellation_after_entries(cancel_after_reads);
            wire::write_frames(&mut writer, vec![wire::simple_query_frame(family.query())]).await;
            let frames = wire::read_frames_until_ready(&mut reader).await;
            set_query_scan_cancellation_after_entries(None);
            let error = frames
                .iter()
                .find(|(tag, _)| *tag == b'E')
                .expect("pgwire analytical error");
            let fields = wire::parse_error_fields(&error.1);
            let sqlstate = error_field(&fields, 'C')
                .expect("SQLSTATE error field")
                .to_string();
            server.stop().await;
            sqlstate
        });
        let _ = std::fs::remove_dir_all(path);
        sqlstate
    }

    fn seed_family(cassie: &Cassie, session: &CassieSession, family: AnalyticalFamily) {
        match family {
            AnalyticalFamily::TimeSeries => seed_time_series(cassie, session),
            AnalyticalFamily::ColumnProjection => {
                seed_column(cassie, session, "controlled_column_projection");
            }
            AnalyticalFamily::ColumnAggregate => {
                seed_column(cassie, session, "controlled_column_aggregate");
            }
            AnalyticalFamily::Graph => seed_graph(cassie, session),
        }
    }

    fn seed_time_series(cassie: &Cassie, session: &CassieSession) {
        execute(
            cassie,
            session,
            "CREATE TABLE controlled_time_series (tenant TEXT, event_at TIMESTAMP, amount INT)",
        );
        execute(
        cassie,
        session,
        "CREATE INDEX controlled_time_series_idx ON controlled_time_series USING time_series (event_at) WITH (bucket_width = '1 hour', partition_by = tenant)",
    );
        let rows = (0..FIXTURE_ROWS)
            .map(|index| {
                let day = 1 + index / 24;
                let hour = index % 24;
                (
                    Some(format!("event-{index:04}")),
                    json!({
                        "tenant": "acme",
                        "event_at": format!("2026-01-{day:02}T{hour:02}:00:00Z"),
                        "amount": index,
                    }),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_time_series_documents("controlled_time_series", rows)
            .expect("seed time-series rows");
    }

    fn seed_column(cassie: &Cassie, session: &CassieSession, table: &str) {
        execute(
            cassie,
            session,
            &format!("CREATE TABLE {table} (score INT, label TEXT)"),
        );
        let rows = (0..FIXTURE_ROWS)
            .map(|index| {
                (
                    Some(format!("row-{index:04}")),
                    json!({
                        "score": index,
                        "label": format!("label-{index:04}-{}", "x".repeat(256)),
                    }),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_documents(table, rows)
            .expect("seed column rows");
        execute(
        cassie,
        session,
        &format!(
            "CREATE INDEX {table}_idx ON {table} USING column (score, label) WITH (segment_size = 1)"
        ),
    );
    }

    fn seed_graph(cassie: &Cassie, session: &CassieSession) {
        execute(cassie, session, "CREATE GRAPH controlled_graph");
        let rows = (0..FIXTURE_ROWS)
            .map(|index| {
                let edge_id = format!("edge-{index:04}");
                (
                    Some(edge_id.clone()),
                    json!({
                        "edge_id": edge_id,
                        "source_type": "person",
                        "source_id": "root",
                        "target_type": "person",
                        "target_id": format!("node-{index:04}-{}", "x".repeat(256)),
                        "edge_type": "knows",
                        "weight": index,
                    }),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_graph_documents("controlled_graph_edges", rows)
            .expect("seed graph edges");
    }

    fn metric(metrics: &serde_json::Value, family: &str, name: &str) -> u64 {
        metrics[family][name].as_u64().unwrap_or_default()
    }

    fn assert_query_cleanup(cassie: &Cassie) {
        let metrics = cassie.metrics();
        assert_eq!(metric(&metrics, "runtime", "running_queries"), 0);
        assert_eq!(
            metric(&metrics, "query", "current_accounted_memory_bytes"),
            0
        );
    }

    fn assert_failed_path_metrics_unchanged(
        family: AnalyticalFamily,
        before: &serde_json::Value,
        after: &serde_json::Value,
    ) {
        let fields: &[(&str, &str)] = match family {
            AnalyticalFamily::TimeSeries => &[
                ("time_series", "scans"),
                ("time_series", "bucket_native_hits"),
                ("time_series", "fallback_scans"),
                ("time_series", "rows"),
                ("time_series", "index_entries_scanned"),
                ("time_series", "row_point_fetches"),
            ],
            AnalyticalFamily::ColumnProjection => &[
                ("column_batches", "scans"),
                ("column_batches", "row_fetches_avoided"),
                ("column_batches", "fallback_scans"),
                ("column_batches", "chunks_read"),
            ],
            AnalyticalFamily::ColumnAggregate => &[
                ("aggregate_acceleration", "scans"),
                ("aggregate_acceleration", "accelerated_segments"),
                ("aggregate_acceleration", "row_blob_fallbacks"),
                ("column_batches", "fallback_scans"),
            ],
            AnalyticalFamily::Graph => &[
                ("graph", "traversals"),
                ("graph", "rows"),
                ("graph", "reads"),
                ("graph", "candidates"),
            ],
        };
        for (metric_family, name) in fields {
            assert_eq!(
                metric(after, metric_family, name),
                metric(before, metric_family, name),
                "{} published failed-path metric {metric_family}.{name}",
                family.label()
            );
        }
        assert_eq!(
            metric(after, "query", "rows_returned_total"),
            metric(before, "query", "rows_returned_total"),
            "{} published partial rows",
            family.label()
        );
    }

    fn exact_fallback_evidence(family: AnalyticalFamily, fixture: &Fixture) -> FallbackEvidence {
        match family {
            AnalyticalFamily::TimeSeries => execute(
                &fixture.cassie,
                &fixture.session,
                "DROP INDEX controlled_time_series_idx ON controlled_time_series",
            ),
            AnalyticalFamily::ColumnProjection => execute(
                &fixture.cassie,
                &fixture.session,
                "DROP INDEX controlled_column_projection_idx ON controlled_column_projection",
            ),
            AnalyticalFamily::ColumnAggregate => execute(
                &fixture.cassie,
                &fixture.session,
                "DROP INDEX controlled_column_aggregate_idx ON controlled_column_aggregate",
            ),
            AnalyticalFamily::Graph => {
                execute(&fixture.cassie, &fixture.session, "BEGIN");
                execute(
                &fixture.cassie,
                &fixture.session,
                "INSERT INTO controlled_graph_edges (edge_id, source_type, source_id, target_type, target_id, edge_type, weight) VALUES ('edge-extra', 'person', 'root', 'person', 'node-extra', 'knows', 0)",
            );
            }
        }
        let rows = fixture
            .cassie
            .execute_sql(&fixture.session, family.query(), vec![])
            .expect("exact controlled fallback")
            .rows;
        if matches!(family, AnalyticalFamily::Graph) {
            let overlay_metrics = fixture.cassie.metrics();
            execute(&fixture.cassie, &fixture.session, "COMMIT");
            let committed = fixture
                .cassie
                .execute_sql(&fixture.session, family.query(), vec![])
                .expect("committed native graph query")
                .rows;
            assert_eq!(rows, committed, "graph overlay/native equivalence");
            return FallbackEvidence {
                rows,
                overlay_metrics: Some(overlay_metrics),
                final_metrics: fixture.cassie.metrics(),
            };
        }
        FallbackEvidence {
            rows,
            overlay_metrics: None,
            final_metrics: fixture.cassie.metrics(),
        }
    }

    fn assert_success_metrics(
        family: AnalyticalFamily,
        before: &serde_json::Value,
        after: &serde_json::Value,
    ) {
        assert_eq!(
            family.successful_paths(after) - family.successful_paths(before),
            2,
            "{} successful path count",
            family.label()
        );
        match family {
            AnalyticalFamily::TimeSeries => {
                assert_eq!(
                    metric(after, "time_series", "bucket_native_hits")
                        - metric(before, "time_series", "bucket_native_hits"),
                    2
                );
                assert!(
                    metric(after, "time_series", "index_entries_scanned")
                        - metric(before, "time_series", "index_entries_scanned")
                        <= (2 * FIXTURE_ROWS) as u64
                );
                assert!(
                    metric(after, "time_series", "row_point_fetches")
                        - metric(before, "time_series", "row_point_fetches")
                        <= (2 * FIXTURE_ROWS) as u64
                );
            }
            AnalyticalFamily::ColumnProjection => {
                assert_eq!(
                    metric(after, "column_batches", "row_fetches_avoided")
                        - metric(before, "column_batches", "row_fetches_avoided"),
                    (2 * FIXTURE_ROWS) as u64
                );
                assert!(
                    metric(after, "column_batches", "chunks_read")
                        - metric(before, "column_batches", "chunks_read")
                        <= (2 * FIXTURE_ROWS * 3) as u64
                );
            }
            AnalyticalFamily::ColumnAggregate => assert_eq!(
                metric(after, "aggregate_acceleration", "accelerated_segments")
                    - metric(before, "aggregate_acceleration", "accelerated_segments"),
                (2 * FIXTURE_ROWS) as u64
            ),
            AnalyticalFamily::Graph => {
                assert!(metric(after, "graph", "last_reads") <= FIXTURE_ROWS as u64);
                assert!(metric(after, "graph", "last_candidates") <= FIXTURE_ROWS as u64);
            }
        }
    }

    #[test]
    fn should_reject_each_analytical_path_atomically_given_the_same_low_memory_budget() {
        let _hook_guard = query_scan_control_test_guard();
        for family in AnalyticalFamily::ALL {
            // Arrange
            let fixture = Fixture::new(family, LOW_MEMORY_BYTES);
            let before = fixture.cassie.metrics();
            let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();

            // Act
            let error = fixture
                .cassie
                .execute_sql(&fixture.session, family.query(), vec![])
                .expect_err("low-budget analytical query should be atomic");
            let after = fixture.cassie.metrics();
            let reads = fixture
                .cassie
                .midge
                .query_scan_entries_for_diagnostics()
                .saturating_sub(before_reads);

            // Assert
            assert!(
                matches!(error, CassieError::ResourceLimit(_)),
                "{} should report SQLSTATE 54000, got {error:?}",
                family.label()
            );
            assert_failed_path_metrics_unchanged(family, &before, &after);
            assert!(
                reads <= family.controlled_read_bound(),
                "{} low-memory read bound: {reads}",
                family.label()
            );
            assert_query_cleanup(&fixture.cassie);
            fixture.cleanup();
            assert_eq!(
                wire_sqlstate(family, LOW_MEMORY_BYTES, None),
                "54000",
                "{} pgwire low-memory SQLSTATE",
                family.label()
            );
        }
    }

    #[test]
    fn should_cancel_each_analytical_path_after_three_controlled_reads_without_partial_metrics() {
        // Arrange
        let _hook_guard = query_scan_control_test_guard();
        let fixtures = AnalyticalFamily::ALL
            .map(|family| (family, Fixture::new(family, NORMAL_MEMORY_BUDGET)));

        // Act
        for (family, fixture) in fixtures {
            let before_metrics = fixture.cassie.metrics();
            let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
            set_query_scan_cancellation_after_entries(Some(3));
            let error = fixture
                .cassie
                .execute_sql(&fixture.session, family.query(), vec![])
                .expect_err("controlled analytical read should cancel");
            set_query_scan_cancellation_after_entries(None);
            let after_metrics = fixture.cassie.metrics();
            let reads = fixture
                .cassie
                .midge
                .query_scan_entries_for_diagnostics()
                .saturating_sub(before_reads);

            // Assert
            assert!(
                matches!(error, CassieError::QueryCancelled),
                "{} should report SQLSTATE 57014, got {error:?}",
                family.label()
            );
            assert_eq!(reads, 3, "{} cancellation boundary", family.label());
            assert_failed_path_metrics_unchanged(family, &before_metrics, &after_metrics);
            assert_query_cleanup(&fixture.cassie);
            fixture.cleanup();
            assert_eq!(
                wire_sqlstate(family, NORMAL_MEMORY_BUDGET, Some(3)),
                "57014",
                "{} pgwire cancellation SQLSTATE",
                family.label()
            );
        }
    }

    #[test]
    fn should_publish_only_deterministic_bounded_final_analytical_paths() {
        let _hook_guard = query_scan_control_test_guard();
        for family in AnalyticalFamily::ALL {
            // Arrange
            let fixture = Fixture::new(family, NORMAL_MEMORY_BUDGET);
            let before_metrics = fixture.cassie.metrics();
            let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();

            // Act
            let first = fixture
                .cassie
                .execute_sql(&fixture.session, family.query(), vec![])
                .expect("first analytical query");
            let second = fixture
                .cassie
                .execute_sql(&fixture.session, family.query(), vec![])
                .expect("second analytical query");
            let selected_metrics = fixture.cassie.metrics();
            let selected_reads = fixture
                .cassie
                .midge
                .query_scan_entries_for_diagnostics()
                .saturating_sub(before_reads);
            let fallback = exact_fallback_evidence(family, &fixture);
            let final_metrics = &fallback.final_metrics;

            // Assert
            assert_eq!(first.rows, second.rows, "{} ordering", family.label());
            if matches!(family, AnalyticalFamily::Graph) {
                assert_ne!(fallback.rows, first.rows, "graph overlay visibility");
                assert!(
                    fallback.rows.iter().any(|row| {
                        row.first() == Some(&cassie::types::Value::String("edge-extra".to_string()))
                    }),
                    "graph overlay row visibility"
                );
            } else {
                assert_eq!(
                    fallback.rows,
                    first.rows,
                    "{} exact fallback",
                    family.label()
                );
            }
            if matches!(family, AnalyticalFamily::ColumnAggregate) {
                assert_eq!(
                    first.rows,
                    vec![vec![
                        cassie::types::Value::Int64(64),
                        cassie::types::Value::Int64(2016),
                        cassie::types::Value::Float64(31.5),
                    ]]
                );
            } else {
                assert_eq!(first.rows.len(), RESULT_LIMIT);
            }
            assert_success_metrics(family, &before_metrics, &selected_metrics);
            assert!(
                selected_reads <= 2 * family.controlled_read_bound(),
                "{} controlled read bound: {selected_reads}",
                family.label()
            );
            if matches!(family, AnalyticalFamily::Graph) {
                let overlay_metrics = fallback
                    .overlay_metrics
                    .as_ref()
                    .expect("graph overlay metrics");
                assert_eq!(
                    metric(overlay_metrics, "graph", "traversals")
                        - metric(&selected_metrics, "graph", "traversals"),
                    1
                );
                assert_eq!(
                    overlay_metrics["graph"]["last_fallback_reason"].as_str(),
                    Some("transaction-overlay")
                );
                assert!(metric(overlay_metrics, "graph", "last_reads") <= 65);
                assert!(metric(overlay_metrics, "graph", "last_candidates") <= 65);
                assert!(
                    metric(overlay_metrics, "graph", "reads")
                        - metric(&selected_metrics, "graph", "reads")
                        <= 65
                );
                assert!(
                    metric(overlay_metrics, "graph", "candidates")
                        - metric(&selected_metrics, "graph", "candidates")
                        <= 65
                );
                assert_eq!(
                    metric(final_metrics, "graph", "traversals")
                        - metric(overlay_metrics, "graph", "traversals"),
                    1
                );
                assert!(metric(final_metrics, "graph", "last_reads") <= FIXTURE_ROWS as u64);
                assert!(metric(final_metrics, "graph", "last_candidates") <= FIXTURE_ROWS as u64);
            } else {
                assert_eq!(
                    family.successful_paths(final_metrics),
                    family.successful_paths(&selected_metrics),
                    "{} fallback published accelerator success",
                    family.label()
                );
            }
            assert_query_cleanup(&fixture.cassie);
            fixture.cleanup();
        }
    }
}

// Formerly tests/specialized_query_controls_retrieval.rs.
mod specialized_query_controls_retrieval {
    use cassie::app::{Cassie, CassieError};
    use cassie::config::{
        CassieRuntimeConfig, EmbeddingsRuntimeConfig, ExecutionResultCacheEnabled,
        LocalRuntimeConfig,
    };
    use cassie::types::Value;

    use super::support_sql as support;

    const COLLECTION: &str = "specialized_retrieval_controls";
    const FIXTURE_ROWS: usize = 64;
    const RESULT_LIMIT: usize = 5;
    const LOW_MEMORY_BYTES: usize = 1_024;

    #[derive(Clone, Copy, Debug)]
    enum RetrievalCase {
        Fulltext,
        VectorExact,
        VectorHnsw,
        VectorIvfFlat,
        Hybrid,
    }

    impl RetrievalCase {
        const ALL: [Self; 5] = [
            Self::Fulltext,
            Self::VectorExact,
            Self::VectorHnsw,
            Self::VectorIvfFlat,
            Self::Hybrid,
        ];

        const fn label(self) -> &'static str {
            match self {
                Self::Fulltext => "fulltext",
                Self::VectorExact => "vector-exact",
                Self::VectorHnsw => "vector-hnsw",
                Self::VectorIvfFlat => "vector-ivfflat",
                Self::Hybrid => "hybrid",
            }
        }

        const fn metric_family(self) -> &'static str {
            match self {
                Self::Fulltext => "search",
                Self::VectorExact | Self::VectorHnsw | Self::VectorIvfFlat => "vector",
                Self::Hybrid => "hybrid",
            }
        }

        const fn controlled_read_bound(self) -> u64 {
            match self {
                Self::Fulltext | Self::VectorIvfFlat => 140,
                Self::VectorExact => FIXTURE_ROWS as u64,
                Self::VectorHnsw => 80,
                Self::Hybrid => 4 * FIXTURE_ROWS as u64 + 8,
            }
        }

        const fn uses_persisted_retrieval(self) -> bool {
            !matches!(self, Self::VectorExact)
        }
    }

    struct RetrievalFixture {
        cassie: Cassie,
        path: String,
        case: RetrievalCase,
    }

    impl RetrievalFixture {
        fn query(&self) -> String {
            match self.case {
                RetrievalCase::Fulltext => format!(
                    "SELECT id, search_score(body, 'alpha') AS score FROM {COLLECTION} \
                 WHERE search(body, 'alpha') ORDER BY score DESC LIMIT {RESULT_LIMIT}"
                ),
                RetrievalCase::VectorExact
                | RetrievalCase::VectorHnsw
                | RetrievalCase::VectorIvfFlat => format!(
                    "SELECT id, vector_distance(embedding, '[0,0,0]') AS distance \
                 FROM {COLLECTION} ORDER BY distance ASC LIMIT {RESULT_LIMIT}"
                ),
                RetrievalCase::Hybrid => format!(
                    "SELECT id, hybrid_score(search_score(body, 'alpha'), \
                 vector_score(embedding, '[0,0,0]')) AS score FROM {COLLECTION} \
                 ORDER BY score DESC LIMIT {RESULT_LIMIT}"
                ),
            }
        }

        fn drop_accelerator(&self) {
            let indexes: &[&str] = match self.case {
                RetrievalCase::Fulltext => &["retrieval_body_fulltext"],
                RetrievalCase::VectorHnsw => &["retrieval_embedding_hnsw"],
                RetrievalCase::VectorIvfFlat => &["retrieval_embedding_ivf"],
                RetrievalCase::Hybrid => &["retrieval_embedding_hnsw", "retrieval_body_fulltext"],
                RetrievalCase::VectorExact => &[],
            };
            for index in indexes {
                self.cassie
                    .execute_sql(
                        &self.cassie.create_session("tester", None),
                        &format!("DROP INDEX {index} ON {COLLECTION}"),
                        vec![],
                    )
                    .expect("drop retrieval accelerator");
            }
        }
    }

    impl Drop for RetrievalFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn fixture(case: RetrievalCase, memory_budget: usize) -> RetrievalFixture {
        support::use_local_storage();
        let path = support::data_dir(&format!("specialized-{}", case.label()));
        let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
        config.limits.query_memory_budget_bytes = memory_budget;
        config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
        config.limits.parallel_scoring_workers = 1;
        config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
            model: "deterministic-test".to_string(),
            dimensions: 3,
        });
        let cassie =
            Cassie::new_with_data_dir_and_config(&path, config).expect("retrieval fixture");
        cassie.startup().expect("startup retrieval fixture");
        let session = cassie.create_session("tester", None);
        cassie
            .execute_sql(
                &session,
                &format!("CREATE TABLE {COLLECTION} (body TEXT, embedding VECTOR(3))"),
                vec![],
            )
            .expect("create retrieval table");
        let rows = (0..FIXTURE_ROWS)
            .map(|index| {
                let coordinate =
                    f64::from(u32::try_from(index).expect("fixture index fits u32")) / 100.0;
                (
                    Some(format!("row-{index:04}")),
                    serde_json::json!({
                        "body": format!(
                            "alpha {} retrieval control row {index:04} {}",
                            "alpha ".repeat(index % 4),
                            "bounded-payload-".repeat(12)
                        ),
                        "embedding": [coordinate, coordinate / 2.0, coordinate / 4.0]
                    }),
                )
            })
            .collect();
        cassie
            .midge
            .put_fresh_documents(COLLECTION, rows)
            .expect("seed exact retrieval fixture");
        if matches!(case, RetrievalCase::Fulltext | RetrievalCase::Hybrid) {
            cassie
                .execute_sql(
                    &session,
                    &format!(
                    "CREATE INDEX retrieval_body_fulltext ON {COLLECTION} USING fulltext (body)"
                ),
                    vec![],
                )
                .expect("create fulltext index");
        }
        match case {
            RetrievalCase::VectorHnsw | RetrievalCase::Hybrid => {
                cassie
                    .execute_sql(
                        &session,
                        &format!(
                            "CREATE INDEX retrieval_embedding_hnsw ON {COLLECTION} USING vector \
                     (embedding) WITH (source_field = body, metric = l2, index_type = hnsw, \
                     m = 8, ef_construction = 64, ef_search = 64)"
                        ),
                        vec![],
                    )
                    .expect("create HNSW index");
            }
            RetrievalCase::VectorIvfFlat => {
                cassie
                    .execute_sql(
                        &session,
                        &format!(
                            "CREATE INDEX retrieval_embedding_ivf ON {COLLECTION} USING vector \
                     (embedding) WITH (source_field = body, metric = l2, index_type = ivfflat, \
                     lists = 4, probes = 4, training_sample_size = 64, training_seed = 7)"
                        ),
                        vec![],
                    )
                    .expect("create IVFFlat index");
            }
            RetrievalCase::Fulltext | RetrievalCase::VectorExact => {}
        }
        RetrievalFixture { cassie, path, case }
    }

    fn metric(metrics: &serde_json::Value, family: &str, name: &str) -> u64 {
        metrics[family][name].as_u64().unwrap_or_default()
    }

    fn assert_query_cleanup(cassie: &Cassie) {
        let metrics = cassie.metrics();
        assert_eq!(metric(&metrics, "runtime", "running_queries"), 0);
        assert_eq!(
            metric(&metrics, "query", "current_accounted_memory_bytes"),
            0
        );
    }

    fn assert_failed_path_metrics_unchanged(
        case: RetrievalCase,
        before: &serde_json::Value,
        after: &serde_json::Value,
    ) {
        let family = case.metric_family();
        for name in [
            "count",
            "candidate_count_total",
            "result_count_total",
            "retrieval_stage_queries_total",
            "posting_reads_total",
            "ann_reads_total",
            "candidate_row_fetches_total",
            "exact_reranks_total",
            "hnsw_executions",
            "hnsw_fallbacks",
            "ivfflat_executions",
            "ivfflat_fallbacks",
            "row_scan_fallback_total",
            "generation_rejections_total",
            "prefilter_input_candidate_count_total",
            "prefilter_filtered_candidate_count_total",
            "prefilter_fallback_count_total",
            "candidate_budget_rejections_total",
            "truncation_count_total",
        ] {
            assert_eq!(
                metric(after, family, name),
                metric(before, family, name),
                "{} published failed-path metric {family}.{name}",
                case.label()
            );
        }
        assert_eq!(
            metric(after, "query", "rows_returned_total"),
            metric(before, "query", "rows_returned_total"),
            "{} published partial rows",
            case.label()
        );
    }

    fn selected_read_count(case: RetrievalCase, metrics: &serde_json::Value) -> u64 {
        match case {
            RetrievalCase::Fulltext => metric(metrics, "search", "posting_reads_total"),
            RetrievalCase::VectorExact => 0,
            RetrievalCase::VectorHnsw | RetrievalCase::VectorIvfFlat => {
                metric(metrics, "vector", "ann_reads_total")
            }
            RetrievalCase::Hybrid => {
                metric(metrics, "hybrid", "posting_reads_total")
                    + metric(metrics, "hybrid", "ann_reads_total")
            }
        }
    }

    fn result_ids(rows: &[Vec<Value>]) -> Vec<&str> {
        rows.iter()
            .map(|row| row[0].as_str().expect("string result id"))
            .collect()
    }

    #[test]
    fn should_reject_every_retrieval_family_given_the_same_low_memory_budget() {
        let _guard = cassie::midge::adapter::query_scan_control_test_guard();
        for case in RetrievalCase::ALL {
            // Arrange
            let fixture = fixture(case, LOW_MEMORY_BYTES);
            let session = fixture.cassie.create_session("reader", None);
            let before = fixture.cassie.metrics();

            // Act
            let error = fixture
                .cassie
                .execute_sql(&session, &fixture.query(), vec![])
                .expect_err("low-budget retrieval should be atomic");
            let after = fixture.cassie.metrics();

            // Assert
            assert!(
                matches!(error, CassieError::ResourceLimit(_)),
                "{} should map to SQLSTATE 54000, got {error:?}",
                case.label()
            );
            assert_failed_path_metrics_unchanged(case, &before, &after);
            assert_query_cleanup(&fixture.cassie);
        }
    }

    #[test]
    fn should_cancel_every_retrieval_family_after_three_controlled_reads() {
        let _guard = cassie::midge::adapter::query_scan_control_test_guard();
        for case in RetrievalCase::ALL {
            // Arrange
            let fixture = fixture(case, 4 * 1024 * 1024);
            let session = fixture.cassie.create_session("reader", None);
            let before_metrics = fixture.cassie.metrics();
            let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
            cassie::midge::adapter::set_query_scan_cancellation_after_entries(Some(3));

            // Act
            let error = fixture
                .cassie
                .execute_sql(&session, &fixture.query(), vec![])
                .expect_err("controlled retrieval should be cancelled");
            cassie::midge::adapter::set_query_scan_cancellation_after_entries(None);
            let after_metrics = fixture.cassie.metrics();
            let reads = fixture
                .cassie
                .midge
                .query_scan_entries_for_diagnostics()
                .saturating_sub(before_reads);

            // Assert
            assert!(
                matches!(error, CassieError::QueryCancelled),
                "{} should map to SQLSTATE 57014, got {error:?}",
                case.label()
            );
            assert_eq!(reads, 3, "{} cancellation boundary", case.label());
            assert_failed_path_metrics_unchanged(case, &before_metrics, &after_metrics);
            assert_query_cleanup(&fixture.cassie);
        }
    }

    #[test]
    fn should_cancel_fulltext_exact_fallback_after_three_controlled_rows() {
        let _guard = cassie::midge::adapter::query_scan_control_test_guard();

        // Arrange
        let fixture = fixture(RetrievalCase::Fulltext, 4 * 1024 * 1024);
        fixture.drop_accelerator();
        let session = fixture.cassie.create_session("reader", None);
        let before_metrics = fixture.cassie.metrics();
        let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
        cassie::midge::adapter::set_query_scan_cancellation_after_entries(Some(3));

        // Act
        let error = fixture
            .cassie
            .execute_sql(&session, &fixture.query(), vec![])
            .expect_err("controlled exact fulltext fallback should be cancelled");
        cassie::midge::adapter::set_query_scan_cancellation_after_entries(None);
        let after_metrics = fixture.cassie.metrics();
        let reads = fixture
            .cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_reads);

        // Assert
        assert!(matches!(error, CassieError::QueryCancelled));
        assert_eq!(reads, 3);
        assert_failed_path_metrics_unchanged(
            RetrievalCase::Fulltext,
            &before_metrics,
            &after_metrics,
        );
        assert_query_cleanup(&fixture.cassie);
    }

    #[test]
    fn should_publish_no_hybrid_diagnostics_when_exact_fallback_is_cancelled() {
        let _guard = cassie::midge::adapter::query_scan_control_test_guard();

        // Arrange
        let fixture = fixture(RetrievalCase::Hybrid, 4 * 1024 * 1024);
        let session = fixture.cassie.create_session("reader", None);
        fixture
            .cassie
            .execute_sql(
                &session,
                &format!("DROP INDEX retrieval_body_fulltext ON {COLLECTION}"),
                vec![],
            )
            .expect("force exact hybrid fallback");
        let before_metrics = fixture.cassie.metrics();
        let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();
        cassie::midge::adapter::set_query_scan_cancellation_after_entries(Some(3));

        // Act
        let error = fixture
            .cassie
            .execute_sql(&session, &fixture.query(), vec![])
            .expect_err("controlled exact hybrid fallback should be cancelled");
        cassie::midge::adapter::set_query_scan_cancellation_after_entries(None);
        let after_metrics = fixture.cassie.metrics();
        let reads = fixture
            .cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_reads);

        // Assert
        assert!(matches!(error, CassieError::QueryCancelled));
        assert_eq!(reads, 3);
        assert_eq!(after_metrics["hybrid"], before_metrics["hybrid"]);
        assert_query_cleanup(&fixture.cassie);
    }

    #[test]
    fn should_reserve_exact_hybrid_prefilter_rows_before_allocation_given_low_memory() {
        let _guard = cassie::midge::adapter::query_scan_control_test_guard();

        // Arrange
        let fixture = fixture(RetrievalCase::Hybrid, LOW_MEMORY_BYTES);
        let session = fixture.cassie.create_session("reader", None);
        fixture
            .cassie
            .execute_sql(
                &session,
                &format!("DROP INDEX retrieval_body_fulltext ON {COLLECTION}"),
                vec![],
            )
            .expect("force exact hybrid fallback");
        let before_metrics = fixture.cassie.metrics();

        // Act
        let error = fixture
            .cassie
            .execute_sql(&session, &fixture.query(), vec![])
            .expect_err("exact hybrid prefilter rows must remain bounded");
        let after_metrics = fixture.cassie.metrics();

        // Assert
        assert!(matches!(error, CassieError::ResourceLimit(_)));
        assert_eq!(after_metrics["hybrid"], before_metrics["hybrid"]);
        assert_query_cleanup(&fixture.cassie);
    }

    #[test]
    fn should_publish_only_deterministic_bounded_final_retrieval_paths() {
        let _guard = cassie::midge::adapter::query_scan_control_test_guard();
        for case in RetrievalCase::ALL {
            // Arrange
            let fixture = fixture(case, 4 * 1024 * 1024);
            let session = fixture.cassie.create_session("reader", None);
            let query = fixture.query();
            let before = fixture.cassie.metrics();
            let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();

            // Act
            let first = fixture
                .cassie
                .execute_sql(&session, &query, vec![])
                .expect("first retrieval query");
            let second = fixture
                .cassie
                .execute_sql(&session, &query, vec![])
                .expect("second retrieval query");
            let selected = fixture.cassie.metrics();
            let controlled_reads = fixture
                .cassie
                .midge
                .query_scan_entries_for_diagnostics()
                .saturating_sub(before_reads);

            // Assert
            assert_eq!(first.rows, second.rows, "{} ordering", case.label());
            assert_eq!(first.rows.len(), RESULT_LIMIT);
            let ids = result_ids(&first.rows);
            assert!(ids.windows(2).all(|pair| pair[0] != pair[1]));
            let family = case.metric_family();
            assert_eq!(
                metric(&selected, family, "count") - metric(&before, family, "count"),
                2
            );
            assert_eq!(
                metric(&selected, family, "result_count_total")
                    - metric(&before, family, "result_count_total"),
                (2 * RESULT_LIMIT) as u64
            );
            assert!(
                metric(&selected, family, "candidate_count_total")
                    - metric(&before, family, "candidate_count_total")
                    <= (2 * FIXTURE_ROWS) as u64,
                "{} candidate bound",
                case.label()
            );
            assert!(
                controlled_reads <= 2 * case.controlled_read_bound(),
                "{} controlled read bound: {controlled_reads}",
                case.label()
            );
            assert_query_cleanup(&fixture.cassie);

            if case.uses_persisted_retrieval() {
                let selected_reads = selected_read_count(case, &selected);
                assert!(
                    selected_reads > selected_read_count(case, &before),
                    "{} selected path should publish reads",
                    case.label()
                );
                fixture.drop_accelerator();
                let fallback = fixture
                    .cassie
                    .execute_sql(&session, &query, vec![])
                    .expect("exact fallback query");
                let after_fallback = fixture.cassie.metrics();
                assert_eq!(fallback.rows, first.rows, "{} exact fallback", case.label());
                assert_eq!(
                    selected_read_count(case, &after_fallback),
                    selected_reads,
                    "{} exact fallback published discarded retrieval reads",
                    case.label()
                );
                assert_query_cleanup(&fixture.cassie);
            }
        }
    }
}
