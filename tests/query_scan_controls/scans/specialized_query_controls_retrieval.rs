use cassie::app::{Cassie, CassieError, CassieSession};
use cassie::config::{
    CassieRuntimeConfig, EmbeddingsRuntimeConfig, ExecutionResultCacheEnabled, LocalRuntimeConfig,
};
use cassie::types::Value;

use super::support_sql as support;

use super::support_retrieval_phase_evidence::{RetrievalPhase, RetrievalPhaseEvidence};

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
    // Fields drop in declaration order; storage owners must precede cleanup.
    cassie: Cassie,
    path: super::support_retrieval_directory::RetrievalDirectory,
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
        let indexes: &[(&str, RetrievalPhase)] = match self.case {
            RetrievalCase::Fulltext => &[("retrieval_body_fulltext", RetrievalPhase::DropFulltext)],
            RetrievalCase::VectorHnsw => &[("retrieval_embedding_hnsw", RetrievalPhase::DropHnsw)],
            RetrievalCase::VectorIvfFlat => {
                &[("retrieval_embedding_ivf", RetrievalPhase::DropIvfFlat)]
            }
            RetrievalCase::Hybrid => &[
                ("retrieval_embedding_hnsw", RetrievalPhase::DropHnsw),
                ("retrieval_body_fulltext", RetrievalPhase::DropFulltext),
            ],
            RetrievalCase::VectorExact => &[],
        };
        for (index, phase) in indexes {
            self.path.evidence().entered(*phase);
            self.cassie
                .execute_sql(
                    &self.cassie.create_session("tester", None),
                    &format!("DROP INDEX {index} ON {COLLECTION}"),
                    vec![],
                )
                .expect("drop retrieval accelerator");
            self.path.evidence().returned(*phase);
        }
    }
}

impl Drop for RetrievalFixture {
    fn drop(&mut self) {
        // Keep the restriction on moving the engine out of the fixture.
        // The engine and directory guard then drop in declaration order.
        self.path
            .evidence()
            .entered(RetrievalPhase::FixtureFieldsRetirement);
    }
}

fn fixture(case: RetrievalCase, memory_budget: usize) -> RetrievalFixture {
    fixture_with_setup(case, memory_budget, |_, _| {})
}

fn fixture_with_setup(
    case: RetrievalCase,
    memory_budget: usize,
    after_setup: impl FnOnce(&Cassie, &str),
) -> RetrievalFixture {
    fixture_with_evidence(
        case,
        memory_budget,
        after_setup,
        RetrievalPhaseEvidence::default(),
    )
}

fn fixture_observed(
    case: RetrievalCase,
    memory_budget: usize,
    evidence: RetrievalPhaseEvidence,
) -> RetrievalFixture {
    fixture_with_evidence(case, memory_budget, |_, _| {}, evidence)
}

fn fixture_with_evidence(
    case: RetrievalCase,
    memory_budget: usize,
    after_setup: impl FnOnce(&Cassie, &str),
    evidence: RetrievalPhaseEvidence,
) -> RetrievalFixture {
    evidence.entered(RetrievalPhase::StorageConfiguration);
    support::use_local_storage();
    let path = support::data_dir(&format!("specialized-{}", case.label()));
    // Construct this before the engine so partial setup unwinds owner first.
    let mut directory = super::support_retrieval_directory::RetrievalDirectory::new(path);
    let mut evidence = evidence.for_path(directory.path());
    directory.set_evidence(evidence.clone());
    let mut config = CassieRuntimeConfig::from_env().expect("runtime config");
    config.limits.query_memory_budget_bytes = memory_budget;
    config.limits.execution_result_cache_enabled = ExecutionResultCacheEnabled::disabled();
    config.limits.parallel_scoring_workers = 1;
    config.embeddings = EmbeddingsRuntimeConfig::Local(LocalRuntimeConfig {
        model: "deterministic-test".to_string(),
        dimensions: 3,
    });
    evidence.returned(RetrievalPhase::StorageConfiguration);
    evidence.not_applicable(RetrievalPhase::ServerTeardown);
    evidence.entered(RetrievalPhase::EngineConstruction);
    let cassie =
        Cassie::new_with_data_dir_and_config(directory.path(), config).expect("retrieval fixture");
    evidence.observe_midge(&cassie.midge);
    directory.set_evidence(evidence.clone());
    evidence.returned(RetrievalPhase::EngineConstruction);
    evidence.entered(RetrievalPhase::Startup);
    cassie.startup().expect("startup retrieval fixture");
    evidence.returned(RetrievalPhase::Startup);
    evidence.entered(RetrievalPhase::SetupSession);
    let session = cassie.create_session("tester", None);
    evidence.returned(RetrievalPhase::SetupSession);
    evidence.entered(RetrievalPhase::CreateTable);
    cassie
        .execute_sql(
            &session,
            &format!("CREATE TABLE {COLLECTION} (body TEXT, embedding VECTOR(3))"),
            vec![],
        )
        .expect("create retrieval table");
    evidence.returned(RetrievalPhase::CreateTable);
    evidence.entered(RetrievalPhase::SeedRows);
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
    evidence.returned(RetrievalPhase::SeedRows);
    create_retrieval_indexes(&cassie, &session, case, &evidence);
    after_setup(&cassie, directory.path());
    RetrievalFixture {
        cassie,
        path: directory,
        case,
    }
}

fn create_retrieval_indexes(
    cassie: &Cassie,
    session: &CassieSession,
    case: RetrievalCase,
    evidence: &RetrievalPhaseEvidence,
) {
    if matches!(case, RetrievalCase::Fulltext | RetrievalCase::Hybrid) {
        evidence.entered(RetrievalPhase::BuildFulltext);
        cassie
            .execute_sql(
                session,
                &format!(
                    "CREATE INDEX retrieval_body_fulltext ON {COLLECTION} USING fulltext (body)"
                ),
                vec![],
            )
            .expect("create fulltext index");
        evidence.returned(RetrievalPhase::BuildFulltext);
    }
    match case {
        RetrievalCase::VectorHnsw | RetrievalCase::Hybrid => {
            evidence.entered(RetrievalPhase::BuildHnsw);
            cassie
                .execute_sql(
                    session,
                    &format!(
                        "CREATE INDEX retrieval_embedding_hnsw ON {COLLECTION} USING vector \
                     (embedding) WITH (source_field = body, metric = l2, index_type = hnsw, \
                     m = 8, ef_construction = 64, ef_search = 64)"
                    ),
                    vec![],
                )
                .expect("create HNSW index");
            evidence.returned(RetrievalPhase::BuildHnsw);
        }
        RetrievalCase::VectorIvfFlat => {
            evidence.entered(RetrievalPhase::BuildIvfFlat);
            cassie
                .execute_sql(
                    session,
                    &format!(
                        "CREATE INDEX retrieval_embedding_ivf ON {COLLECTION} USING vector \
                     (embedding) WITH (source_field = body, metric = l2, index_type = ivfflat, \
                     lists = 4, probes = 4, training_sample_size = 64, training_seed = 7)"
                    ),
                    vec![],
                )
                .expect("create IVFFlat index");
            evidence.returned(RetrievalPhase::BuildIvfFlat);
        }
        RetrievalCase::Fulltext | RetrievalCase::VectorExact => {}
    }
}

fn metric(metrics: &serde_json::Value, family: &str, name: &str) -> u64 {
    metrics[family][name].as_u64().unwrap_or_default()
}

fn assert_query_execution_retired(cassie: &Cassie) -> serde_json::Value {
    let metrics = cassie.metrics();
    assert_eq!(metric(&metrics, "runtime", "running_queries"), 0);
    metrics
}

fn assert_query_cleanup(cassie: &Cassie) {
    let metrics = assert_query_execution_retired(cassie);
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

fn assert_selected_metrics(
    case: RetrievalCase,
    before: &serde_json::Value,
    selected: &serde_json::Value,
    controlled_reads: u64,
) {
    let family = case.metric_family();
    assert_eq!(
        metric(selected, family, "count") - metric(before, family, "count"),
        2
    );
    assert_eq!(
        metric(selected, family, "result_count_total")
            - metric(before, family, "result_count_total"),
        (2 * RESULT_LIMIT) as u64
    );
    assert!(
        metric(selected, family, "candidate_count_total")
            - metric(before, family, "candidate_count_total")
            <= (2 * FIXTURE_ROWS) as u64,
        "{} candidate bound",
        case.label()
    );
    assert!(
        controlled_reads <= 2 * case.controlled_read_bound(),
        "{} controlled read bound: {controlled_reads}",
        case.label()
    );
}

fn run_selected_retrieval_paths(caller: &'static str) -> Vec<String> {
    let invocation = RetrievalPhaseEvidence::new(caller);
    invocation.entered(RetrievalPhase::ThreadLocalGuard);
    let _guard = cassie::midge::adapter::query_scan_control_test_guard();
    invocation.returned(RetrievalPhase::ThreadLocalGuard);
    let mut paths = Vec::new();
    for case in RetrievalCase::ALL {
        // Arrange
        let fixture = fixture_observed(case, 4 * 1024 * 1024, invocation.for_case(case.label()));
        paths.push(fixture.path.path().to_owned());
        let evidence = fixture.path.evidence().clone();
        let _query_locals = evidence.query_locals_scope();
        evidence.entered(RetrievalPhase::ReaderCreation);
        let session = fixture.cassie.create_session("reader", None);
        evidence.returned(RetrievalPhase::ReaderCreation);
        evidence.entered(RetrievalPhase::QueryCreation);
        let query = fixture.query();
        evidence.returned(RetrievalPhase::QueryCreation);
        let before = fixture.cassie.metrics();
        let before_reads = fixture.cassie.midge.query_scan_entries_for_diagnostics();

        // Act
        evidence.entered(RetrievalPhase::SelectedFirst);
        let first = fixture
            .cassie
            .execute_sql(&session, &query, vec![])
            .expect("first retrieval query");
        evidence.returned(RetrievalPhase::SelectedFirst);
        evidence.entered(RetrievalPhase::SelectedSecond);
        let second = fixture
            .cassie
            .execute_sql(&session, &query, vec![])
            .expect("second retrieval query");
        evidence.returned(RetrievalPhase::SelectedSecond);
        let selected = fixture.cassie.metrics();
        let controlled_reads = fixture
            .cassie
            .midge
            .query_scan_entries_for_diagnostics()
            .saturating_sub(before_reads);

        // Assert
        evidence.entered(RetrievalPhase::Assertions);
        assert_eq!(first.rows(), second.rows(), "{} ordering", case.label());
        assert_eq!(first.rows().len(), RESULT_LIMIT);
        let ids = result_ids(first.rows());
        assert!(ids.windows(2).all(|pair| pair[0] != pair[1]));
        assert_selected_metrics(case, &before, &selected, controlled_reads);
        let retained = assert_query_execution_retired(&fixture.cassie);
        assert!(metric(&retained, "query", "current_accounted_memory_bytes") > 0);

        if case.uses_persisted_retrieval() {
            let selected_reads = selected_read_count(case, &selected);
            assert!(
                selected_reads > selected_read_count(case, &before),
                "{} selected path should publish reads",
                case.label()
            );
            fixture.drop_accelerator();
            evidence.entered(RetrievalPhase::ExactFallback);
            let fallback = fixture
                .cassie
                .execute_sql(&session, &query, vec![])
                .expect("exact fallback query");
            evidence.returned(RetrievalPhase::ExactFallback);
            let after_fallback = fixture.cassie.metrics();
            assert_eq!(
                fallback.rows(),
                first.rows(),
                "{} exact fallback",
                case.label()
            );
            assert_eq!(
                selected_read_count(case, &after_fallback),
                selected_reads,
                "{} exact fallback published discarded retrieval reads",
                case.label()
            );
            drop(fallback);
        }
        evidence.returned(RetrievalPhase::Assertions);
        evidence.entered(RetrievalPhase::QueryLocalsRetirement);
        drop((first, second));
        assert_query_cleanup(&fixture.cassie);
    }
    paths
}
#[test]
fn should_publish_only_deterministic_bounded_final_retrieval_paths() {
    run_selected_retrieval_paths("selected_retrieval_paths");
}

#[test]
fn should_drop_retrieval_fixture_owners_without_shutdown_timeout() {
    // Arrange
    let mut paths = Vec::new();

    // Act
    let warnings = super::support_teardown_evidence::capture_warnings(|| {
        paths = run_selected_retrieval_paths("fixture_teardown_warnings");
    });

    // Assert
    assert_eq!(paths.len(), RetrievalCase::ALL.len());
    for path in paths {
        assert!(
            !std::path::Path::new(&path).exists(),
            "retrieval fixture directory survived teardown: {path}"
        );
    }
    assert!(
        !warnings.contains("Midge graceful shutdown did not complete"),
        "retrieval teardown warnings: {warnings}"
    );
}

#[test]
fn should_clean_retrieval_directory_when_setup_unwinds() {
    // Arrange
    let mut path = String::new();
    let mut outcome = Ok(());

    // Act
    let warnings = super::support_teardown_evidence::capture_warnings(|| {
        outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            fixture_with_setup(
                RetrievalCase::VectorHnsw,
                4 * 1024 * 1024,
                |_, directory| {
                    path = directory.to_owned();
                    panic!("controlled retrieval setup panic");
                },
            );
        }));
    });

    // Assert
    let error = outcome.expect_err("setup must unwind");
    assert_eq!(
        error.downcast_ref::<&str>(),
        Some(&"controlled retrieval setup panic")
    );
    assert_ne!(path, "");
    assert!(!std::path::Path::new(&path).exists());
    assert!(
        !warnings.contains("Midge graceful shutdown did not complete"),
        "setup unwind warnings: {warnings}"
    );
}

#[test]
fn should_release_retrieval_engine_while_cloned_reader_results_remain_live() {
    // Arrange
    let invocation = RetrievalPhaseEvidence::new("escaped_reader_results");
    let fixture = fixture_observed(
        RetrievalCase::VectorHnsw,
        4 * 1024 * 1024,
        invocation.for_case(RetrievalCase::VectorHnsw.label()),
    );
    let evidence = fixture.path.evidence().clone();
    let path = fixture.path.path().to_owned();
    evidence.entered(RetrievalPhase::ReaderCreation);
    let reader = fixture.cassie.create_session("reader", None);
    let escaped_reader = reader.clone();
    evidence.returned(RetrievalPhase::ReaderCreation);
    evidence.entered(RetrievalPhase::SelectedFirst);
    let result = fixture
        .cassie
        .execute_sql(&reader, &fixture.query(), vec![])
        .expect("reader retrieval");
    evidence.returned(RetrievalPhase::SelectedFirst);
    let retained = assert_query_execution_retired(&fixture.cassie);
    assert!(metric(&retained, "query", "current_accounted_memory_bytes") > 0);
    drop(reader);

    // Act
    evidence.entered(RetrievalPhase::EscapedReaderResults);
    let warnings = super::support_teardown_evidence::capture_warnings(|| drop(fixture));

    // Assert
    evidence.entered(RetrievalPhase::Assertions);
    assert_eq!(escaped_reader.user, "reader");
    assert_eq!(result.rows().len(), RESULT_LIMIT);
    assert!(!std::path::Path::new(&path).exists());
    assert!(
        !warnings.contains("Midge graceful shutdown did not complete"),
        "escaped reader teardown warnings: {warnings}"
    );
    evidence.returned(RetrievalPhase::Assertions);
    drop(result);
    drop(escaped_reader);
    evidence.returned(RetrievalPhase::EscapedReaderResults);
}
