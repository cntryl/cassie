use std::time::{Duration, Instant};

use cassie::types::{Value, Vector};

const BENCHMARK: &str = "tier3_system_query";
const FIXTURE_SCALE: &str = "100k";
const FIXTURE_ROWS: usize = 100_000;
const RELATIONAL_SQL: &str = "SELECT id FROM bench_documents WHERE status = $1 AND score >= $2 ORDER BY status DESC, score ASC LIMIT 50";
const COLUMN_SQL: &str = "SELECT COUNT(*) AS rows, SUM(score) AS score_sum, AVG(score) AS score_avg FROM bench_documents";
const FULLTEXT_SQL: &str = "SELECT id, search_score(body, $1) AS score FROM bench_documents WHERE search(body, $1) ORDER BY score DESC LIMIT 20";
const VECTOR_SQL: &str = "SELECT id, vector_distance(embedding, $1) AS distance FROM bench_documents ORDER BY distance ASC LIMIT 20";
const HYBRID_SQL: &str = "SELECT id, hybrid_score(search_score(body, $1), vector_score(embedding, $2)) AS score FROM bench_documents ORDER BY score DESC LIMIT 20";
const JOIN_SQL: &str = "SELECT bench_join_users.name, bench_join_orders.total FROM bench_join_users JOIN bench_join_orders ON bench_join_users.user_key = bench_join_orders.order_user_key LIMIT 50";
const GRAPH_SQL: &str = "SELECT node_id FROM graph_expand($1, $2, $3, $4, $5, $6, $7)";
const TIME_SERIES_SQL: &str = "SELECT tenant, amount FROM bench_time_series_events WHERE event_at >= $1 AND event_at < $2 ORDER BY event_at LIMIT 512";

#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier3, BENCHMARK);
    let core_cases = CoreCases::select(&runner);
    let join_case = selected_case(&runner, "vectorized_join_query");
    let graph_case = selected_case(&runner, "graph_expand_query");
    let time_series_case = selected_case(&runner, "time_series_window_scan");
    if !core_cases.any_enabled()
        && join_case.is_none()
        && graph_case.is_none()
        && time_series_case.is_none()
    {
        runner.finish();
        return;
    }

    workloads::configure_tier3_environment();
    let runtime = workloads::runtime();
    let fixture_setup_started = Instant::now();
    let context = runtime
        .block_on(workloads::tier3_query_context(
            "tier3-query-100k",
            FIXTURE_ROWS,
        ))
        .expect("Tier 3 shared query fixture");
    workloads::assert_fixture_boundaries(&context, &context.collection, "doc-0", "doc-99999");
    if core_cases.column.is_some() {
        execute_ddl(
            &context,
            "CREATE INDEX bench_documents_column_idx ON bench_documents USING column (title, body, status, score) WITH (segment_size = 256)",
        );
    }
    workloads::prepare_tier3_query_domains(
        &context,
        FIXTURE_ROWS,
        workloads::Tier3QueryDomains {
            join: join_case.is_some(),
            graph: graph_case.is_some(),
            time_series: time_series_case.is_some(),
        },
    )
    .expect("prepare Tier 3 query domains in shared fixture");
    let fixture_setup = fixture_setup_started.elapsed();
    workloads::assert_result_cache_disabled(&context);

    bench_core_representatives(&mut runner, &context, fixture_setup, core_cases);
    bench_join_representative(&mut runner, &context, fixture_setup, join_case);
    bench_graph_representative(&mut runner, &context, fixture_setup, graph_case);
    bench_time_series_representative(&mut runner, &context, fixture_setup, time_series_case);
    workloads::assert_result_cache_disabled(&context);

    let data_dir = context.data_dir.clone();
    context.cassie.shutdown();
    drop(context);
    runner.finish();
    if data_dir.is_dir() {
        std::fs::remove_dir_all(&data_dir).expect("clean up Tier 3 query fixture directory");
    } else if data_dir.exists() {
        std::fs::remove_file(&data_dir).expect("clean up Tier 3 query fixture marker");
    }
    assert!(!data_dir.exists(), "Tier 3 query fixture cleanup");
}

struct CoreCases {
    relational: Option<stress::StressCase>,
    column: Option<stress::StressCase>,
    fulltext: Option<stress::StressCase>,
    vector_exact: Option<stress::StressCase>,
    vector_hnsw: Option<stress::StressCase>,
    vector_ivf: Option<stress::StressCase>,
    hybrid: Option<stress::StressCase>,
}

impl CoreCases {
    fn select(runner: &stress::CassieStressRunner) -> Self {
        Self {
            relational: selected_case(runner, "mixed_order_scalar_query"),
            column: selected_case(runner, "column_batch_query"),
            fulltext: selected_case(runner, "full_text_query"),
            vector_exact: selected_case(runner, "vector_exact_query"),
            vector_hnsw: selected_case(runner, "vector_hnsw_persisted"),
            vector_ivf: selected_case(runner, "vector_ivfflat_persisted"),
            hybrid: selected_case(runner, "hybrid_query"),
        }
    }

    fn any_enabled(&self) -> bool {
        self.relational.is_some()
            || self.column.is_some()
            || self.fulltext.is_some()
            || self.vector_exact.is_some()
            || self.vector_hnsw.is_some()
            || self.vector_ivf.is_some()
            || self.hybrid.is_some()
    }
}

fn bench_core_representatives(
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    fixture_setup: Duration,
    cases: CoreCases,
) {
    if !cases.any_enabled() {
        return;
    }

    if let Some(case) = cases.relational {
        let case_setup = Instant::now();
        let preflight = workloads::assert_explain_contains(
            context,
            RELATIONAL_SQL,
            relational_params(),
            "bench_documents_status_score_idx",
        );
        let case = evidenced(
            case,
            context,
            fixture_setup + case_setup.elapsed(),
            preflight,
        );
        runner.measure_batch(case, 1, || {
            workloads::execute_expected_query(context, RELATIONAL_SQL, relational_params(), 50)
        });
    }

    if let Some(case) = cases.column {
        let case_setup = Instant::now();
        let preflight = workloads::assert_explain_contains(
            context,
            COLUMN_SQL,
            vec![],
            "aggregate_acceleration=true",
        );
        let case = evidenced(
            case,
            context,
            fixture_setup + case_setup.elapsed(),
            preflight,
        );
        let before = context.cassie.metrics();
        runner.measure_batch(case, 1, || {
            workloads::execute_expected_query(context, COLUMN_SQL, vec![], 1)
        });
        let after = context.cassie.metrics();
        assert_metric_increased(&before, &after, "aggregate_acceleration", "scans");
    }

    if let Some(case) = cases.fulltext {
        let case_setup = Instant::now();
        let preflight = workloads::assert_explain_contains(
            context,
            FULLTEXT_SQL,
            fulltext_params(),
            "collection=postgres.public.bench_documents",
        );
        let case = evidenced(
            case,
            context,
            fixture_setup + case_setup.elapsed(),
            preflight,
        );
        let before = context.cassie.metrics();
        runner.measure_batch(case, 1, || {
            workloads::execute_expected_query(context, FULLTEXT_SQL, fulltext_params(), 20)
        });
        let after = context.cassie.metrics();
        assert_metric_increased(&before, &after, "search", "posting_reads_total");
        assert_metric_unchanged(&before, &after, "search", "row_scan_fallback_total");
    }

    bench_vector_exact_representative(runner, context, fixture_setup, cases.vector_exact);

    bench_indexed_vector_representatives(
        runner,
        context,
        fixture_setup,
        cases.hybrid,
        cases.vector_hnsw,
        cases.vector_ivf,
    );
}

fn bench_vector_exact_representative(
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    fixture_setup: Duration,
    case: Option<stress::StressCase>,
) {
    let Some(case) = case else {
        return;
    };
    let case_setup = Instant::now();
    let preflight = workloads::assert_vector_preflight(
        context,
        VECTOR_SQL,
        vector_params(),
        "collection=postgres.public.bench_documents",
        FIXTURE_ROWS,
        workloads::VectorAccessPath::Exact,
    );
    let case = evidenced(
        case,
        context,
        fixture_setup + case_setup.elapsed(),
        preflight,
    );
    let before = context.cassie.metrics();
    runner.measure_batch(case, 1, || {
        workloads::execute_expected_query(context, VECTOR_SQL, vector_params(), 20)
    });
    let after = context.cassie.metrics();
    assert_metric_increased(&before, &after, "vector", "count");
    assert_metric_unchanged(&before, &after, "vector", "hnsw_executions");
    assert_metric_unchanged(&before, &after, "vector", "ivfflat_executions");
}

fn bench_indexed_vector_representatives(
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    fixture_setup: Duration,
    hybrid: Option<stress::StressCase>,
    hnsw: Option<stress::StressCase>,
    ivf: Option<stress::StressCase>,
) {
    let mut vector_index = None;
    if let Some(case) = hybrid {
        let case_setup = Instant::now();
        install_vector_index(context, &mut vector_index, VectorIndexKind::Hnsw);
        let preflight = workloads::assert_explain_contains(
            context,
            HYBRID_SQL,
            hybrid_params(),
            "mixed_execution=true",
        );
        let case = evidenced(
            case,
            context,
            fixture_setup + case_setup.elapsed(),
            preflight,
        );
        let before = context.cassie.metrics();
        runner.measure_batch(case, 1, || {
            workloads::execute_expected_query(context, HYBRID_SQL, hybrid_params(), 20)
        });
        let after = context.cassie.metrics();
        assert_metric_increased(&before, &after, "hybrid", "posting_reads_total");
        assert_metric_increased(&before, &after, "hybrid", "ann_reads_total");
        assert_metric_increased(&before, &after, "hybrid", "exact_reranks_total");
        assert_metric_unchanged(&before, &after, "hybrid", "prefilter_fallback_count_total");
    }

    if let Some(case) = hnsw {
        let case_setup = Instant::now();
        if !matches!(vector_index, Some(VectorIndexKind::Hnsw)) {
            install_vector_index(context, &mut vector_index, VectorIndexKind::Hnsw);
        }
        bench_ann_case(
            runner,
            case,
            context,
            fixture_setup + case_setup.elapsed(),
            workloads::VectorAccessPath::Hnsw,
            "hnsw_executions",
            "hnsw_fallbacks",
        );
    }
    if let Some(case) = ivf {
        let case_setup = Instant::now();
        install_vector_index(context, &mut vector_index, VectorIndexKind::IvfFlat);
        bench_ann_case(
            runner,
            case,
            context,
            fixture_setup + case_setup.elapsed(),
            workloads::VectorAccessPath::IvfFlat,
            "ivfflat_executions",
            "ivfflat_fallbacks",
        );
    }
}

fn bench_ann_case(
    runner: &mut stress::CassieStressRunner,
    case: stress::StressCase,
    context: &workloads::BenchContext,
    setup_time: Duration,
    access_path: workloads::VectorAccessPath,
    execution_metric: &str,
    fallback_metric: &str,
) {
    let preflight_started = Instant::now();
    let preflight = workloads::assert_vector_preflight(
        context,
        VECTOR_SQL,
        vector_params(),
        "collection=postgres.public.bench_documents",
        FIXTURE_ROWS,
        access_path,
    );
    let case = evidenced(
        case,
        context,
        setup_time + preflight_started.elapsed(),
        preflight,
    );
    let before = context.cassie.metrics();
    runner.measure_batch(case, 1, || {
        workloads::execute_expected_query(context, VECTOR_SQL, vector_params(), 20)
    });
    let after = context.cassie.metrics();
    assert_metric_increased(&before, &after, "vector", execution_metric);
    assert_metric_unchanged(&before, &after, "vector", fallback_metric);
}

fn bench_join_representative(
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    fixture_setup: Duration,
    case: Option<stress::StressCase>,
) {
    let Some(case) = case else {
        return;
    };
    let case_setup = Instant::now();
    workloads::assert_fixture_boundaries(context, "bench_join_users", "user-0", "user-99999");
    workloads::assert_fixture_boundaries(context, "bench_join_orders", "order-0", "order-99999");
    let preflight = workloads::assert_explain_contains(
        context,
        JOIN_SQL,
        vec![],
        "vectorized_join_candidate=true",
    );
    let case = evidenced(
        case,
        context,
        fixture_setup + case_setup.elapsed(),
        preflight,
    );
    let before = context.cassie.metrics();
    runner.measure_batch(case, 1, || {
        workloads::execute_expected_query(context, JOIN_SQL, vec![], 50)
    });
    let after = context.cassie.metrics();
    assert_metric_increased(&before, &after, "joins", "vectorized_joins");
    assert_metric_unchanged(&before, &after, "joins", "vectorized_fallbacks");
}

fn bench_graph_representative(
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    fixture_setup: Duration,
    case: Option<stress::StressCase>,
) {
    let Some(case) = case else {
        return;
    };
    let case_setup = Instant::now();
    workloads::assert_fixture_boundaries(context, "bench_graph_nodes", "node-0", "node-99999");
    let preflight =
        workloads::assert_explain_contains(context, GRAPH_SQL, graph_params(), "operators=");
    let case = evidenced(
        case,
        context,
        fixture_setup + case_setup.elapsed(),
        preflight,
    );
    let before = context.cassie.metrics();
    runner.measure_batch(case, 1, || {
        workloads::execute_expected_query(context, GRAPH_SQL, graph_params(), 4)
    });
    let after = context.cassie.metrics();
    assert_metric_increased(&before, &after, "graph", "traversals");
}

fn bench_time_series_representative(
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    fixture_setup: Duration,
    case: Option<stress::StressCase>,
) {
    let Some(case) = case else {
        return;
    };
    let case_setup = Instant::now();
    workloads::assert_fixture_boundaries(
        context,
        "bench_time_series_events",
        "ts-doc-0",
        "ts-doc-99999",
    );
    let preflight = workloads::assert_explain_contains(
        context,
        TIME_SERIES_SQL,
        time_series_params(),
        "time_series=bucket_width:1 hour",
    );
    let case = evidenced(
        case,
        context,
        fixture_setup + case_setup.elapsed(),
        preflight,
    );
    let before = context.cassie.metrics();
    runner.measure_batch(case, 1, || {
        workloads::execute_expected_query(context, TIME_SERIES_SQL, time_series_params(), 512)
    });
    let after = context.cassie.metrics();
    assert_metric_increased(&before, &after, "time_series", "bucket_native_hits");
    assert_metric_unchanged(&before, &after, "time_series", "fallback_scans");
}

fn selected_case(
    runner: &stress::CassieStressRunner,
    workload: &'static str,
) -> Option<stress::StressCase> {
    let case = stress::StressCase::new(workload, FIXTURE_SCALE)
        .runtime_contract(
            stress::FixtureDeclaration::new(
                performance_benchmarks::FixtureClass::Representative,
                FIXTURE_ROWS,
                "tier3_system_query/100k",
            ),
            stress::OperationUnit::Query,
        )
        .metadata(
            "query_memory_budget_bytes",
            workloads::ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES.to_string(),
        );
    runner.is_enabled(&case).then_some(case)
}

fn evidenced(
    case: stress::StressCase,
    context: &workloads::BenchContext,
    setup_time: Duration,
    preflight: workloads::QueryPreflightEvidence,
) -> stress::StressCase {
    case.metadata("setup_time_ns", setup_time.as_nanos().to_string())
        .preflight_evidence(preflight.selected_access_path, preflight.fallback_reason)
        .runtime_evidence(context.cassie.clone())
}

fn execute_ddl(context: &workloads::BenchContext, sql: &str) {
    context
        .cassie
        .execute_sql(&context.session, sql, vec![])
        .expect("Tier 3 fixture DDL");
}

#[derive(Clone, Copy)]
enum VectorIndexKind {
    Hnsw,
    IvfFlat,
}

impl VectorIndexKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Hnsw => "bench_documents_embedding_hnsw_idx",
            Self::IvfFlat => "bench_documents_embedding_ivf_idx",
        }
    }

    const fn create_sql(self) -> &'static str {
        match self {
            Self::Hnsw => "CREATE INDEX bench_documents_embedding_hnsw_idx ON bench_documents USING vector (embedding) WITH (source_field = body, metric = l2, index_type = hnsw, m = 32, ef_construction = 256, ef_search = 256)",
            Self::IvfFlat => "CREATE INDEX bench_documents_embedding_ivf_idx ON bench_documents USING vector (embedding) WITH (source_field = body, metric = l2, index_type = ivfflat, lists = 64, probes = 16, training_sample_size = 4096, training_seed = 42)",
        }
    }
}

fn install_vector_index(
    context: &workloads::BenchContext,
    current: &mut Option<VectorIndexKind>,
    next: VectorIndexKind,
) {
    if let Some(index) = current.take() {
        let drop_sql = match index {
            VectorIndexKind::Hnsw => {
                "DROP INDEX bench_documents_embedding_hnsw_idx ON bench_documents"
            }
            VectorIndexKind::IvfFlat => {
                "DROP INDEX bench_documents_embedding_ivf_idx ON bench_documents"
            }
        };
        execute_ddl(context, drop_sql);
    }
    execute_ddl(context, next.create_sql());
    assert!(
        context
            .cassie
            .catalog
            .get_index("bench_documents", next.name())
            .is_some(),
        "Tier 3 vector index must be registered before measurement"
    );
    *current = Some(next);
}

fn relational_params() -> Vec<Value> {
    vec![Value::String("approved".to_string()), Value::Int64(10)]
}

fn fulltext_params() -> Vec<Value> {
    vec![Value::String("alpha".to_string())]
}

fn vector_params() -> Vec<Value> {
    vec![Value::Vector(Vector::new(vec![1.0, 0.0, 0.0]))]
}

fn hybrid_params() -> Vec<Value> {
    vec![
        Value::String("alpha delta".to_string()),
        Value::Vector(Vector::new(vec![1.0, 0.0, 0.0])),
    ]
}

fn graph_params() -> Vec<Value> {
    vec![
        Value::String("bench_graph".to_string()),
        Value::String("doc".to_string()),
        Value::String("node-0".to_string()),
        Value::Int64(4),
        Value::String("out".to_string()),
        Value::String("links".to_string()),
        Value::Int64(64),
    ]
}

fn time_series_params() -> Vec<Value> {
    vec![
        Value::String("2026-01-10T00:00:00Z".to_string()),
        Value::String("2026-01-12T00:00:00Z".to_string()),
    ]
}

fn metric(snapshot: &serde_json::Value, section: &str, key: &str) -> u64 {
    snapshot[section][key].as_u64().unwrap_or_default()
}

fn assert_metric_increased(
    before: &serde_json::Value,
    after: &serde_json::Value,
    section: &str,
    key: &str,
) {
    assert!(
        metric(after, section, key) > metric(before, section, key),
        "Tier 3 metric {section}.{key} did not increase"
    );
}

fn assert_metric_unchanged(
    before: &serde_json::Value,
    after: &serde_json::Value,
    section: &str,
    key: &str,
) {
    assert_eq!(
        metric(after, section, key),
        metric(before, section, key),
        "Tier 3 metric {section}.{key} reported a fallback"
    );
}
