use cassie::app::{Cassie, CassieError};
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, ExecutionResultCacheEnabled, LocalRuntimeConfig,
};
use cassie::types::Value;

#[path = "../support/sql.rs"]
mod support;

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
    support::with_fallback();
    let path = support::data_dir(&format!("specialized-{}", case.label()));
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_memory_budget_bytes = memory_budget;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.parallel_scoring_workers = 1;
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "deterministic-test".to_string(),
        dimensions: 3,
    });
    let cassie = Cassie::new_with_data_dir_and_config(&path, config).expect("retrieval fixture");
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
    assert_failed_path_metrics_unchanged(RetrievalCase::Fulltext, &before_metrics, &after_metrics);
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
