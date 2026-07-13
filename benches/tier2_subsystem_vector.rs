const BENCHMARK: &str = "tier2_subsystem_vector";
const QUERY_BATCH: u64 = 64;
const BRUTE_FORCE_BATCH: u64 = 128;
const HNSW_BATCH: u64 = 512;
const IVFFLAT_PROBE_BATCH: u64 = 16_384;
const VECTOR_RECORDS: usize = 1_024;
const IVFFLAT_LISTS: usize = 16;

use std::collections::BTreeMap;

use cassie::embeddings::{
    DistanceMetric, HnswIndexOptions, IvfFlatTrainingState, NormalizedVectorRecord,
};
use cassie::types::Value;

#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let mut runner = stress::runner(BENCHMARK);

    for (dataset, rows) in [("10k", 10_000), ("100k", 100_000), ("250k", 250_000)] {
        let benchmark =
            performance_benchmarks::expect_benchmark(BENCHMARK, "vector_executor", dataset);
        let case =
            stress::StressCase::fixed_operations(2, benchmark.workload, benchmark.fixture_scale);
        if !runner.is_enabled(&case) {
            continue;
        }
        let context = runtime
            .block_on(workloads::unindexed_context(
                &format!("tier2-vector-{dataset}"),
                rows,
            ))
            .expect("benchmark context");
        runner.fixed_timed_count(
            case.metadata("operation_unit", "query"),
            QUERY_BATCH,
            || {
                run_sql_batch(
                    &runtime,
                    &context,
                    "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM bench_documents ORDER BY distance ASC LIMIT 20",
                    QUERY_BATCH,
                )
            },
        );
    }
    bench_candidate_paths(&mut runner);
    bench_persisted_ann_paths(&runtime, &mut runner);

    runner.finish();
}

fn run_sql_batch(
    runtime: &tokio::runtime::Runtime,
    context: &workloads::BenchContext,
    sql: &str,
    queries: u64,
) -> usize {
    let mut rows = 0usize;
    for _ in 0..queries {
        rows = rows.saturating_add(runtime.block_on(workloads::execute_sql(context, sql)));
    }
    std::hint::black_box(rows)
}

fn bench_candidate_paths(runner: &mut stress::CassieStressRunner) {
    let brute_force =
        stress::StressCase::fixed_operations(2, "vector_bruteforce_candidates", "10k")
            .metadata("operation_unit", "candidate_search");
    let hnsw = stress::StressCase::fixed_operations(2, "vector_hnsw_candidates", "10k")
        .metadata("operation_unit", "candidate_search");
    let ivfflat = stress::StressCase::fixed_operations(2, "vector_ivfflat_probe_lists", "10k")
        .metadata("operation_unit", "probe_selection");

    if !runner.is_enabled(&brute_force) && !runner.is_enabled(&hnsw) && !runner.is_enabled(&ivfflat)
    {
        return;
    }

    let fixture = VectorCandidateFixture::new();
    fixture.assert_preflight();

    runner.fixed_timed_count(brute_force, BRUTE_FORCE_BATCH, || {
        fixture.bruteforce_batch(BRUTE_FORCE_BATCH)
    });
    runner.fixed_timed_count(hnsw, HNSW_BATCH, || fixture.hnsw_batch(HNSW_BATCH));
    runner.fixed_timed_count(ivfflat, IVFFLAT_PROBE_BATCH, || {
        fixture.ivfflat_probe_batch(IVFFLAT_PROBE_BATCH)
    });
}

fn bench_persisted_ann_paths(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
) {
    for (dataset, rows) in [("10k", 10_000), ("100k", 100_000), ("250k", 250_000)] {
        bench_persisted_ann_path(runtime, runner, dataset, rows, "hnsw");
        bench_persisted_ann_path(runtime, runner, dataset, rows, "ivfflat");
    }
}

fn bench_persisted_ann_path(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    dataset: &str,
    rows: usize,
    index_type: &str,
) {
    let workload = match index_type {
        "hnsw" => "vector_hnsw_persisted",
        "ivfflat" => "vector_ivfflat_persisted",
        _ => unreachable!("benchmark index type is fixed"),
    };
    let benchmark = performance_benchmarks::expect_benchmark(BENCHMARK, workload, dataset);
    let case = stress::StressCase::fixed_operations(2, benchmark.workload, benchmark.fixture_scale);
    if !runner.is_enabled(&case) {
        return;
    }

    let context = runtime
        .block_on(workloads::context_with_mock_tei_embeddings(
            &format!("tier2-vector-{index_type}-{dataset}"),
            rows,
        ))
        .expect("persisted ANN benchmark context");
    let statement = match index_type {
        "hnsw" => {
            context
                .cassie
                .execute_sql(
                    &context.session,
                    &format!(
                        "DROP INDEX {}_embedding_idx ON {}",
                        context.collection, context.collection
                    ),
                    vec![],
                )
                .expect("drop default vector benchmark index");
            format!(
                "CREATE INDEX {}_embedding_hnsw_idx ON {} USING vector (embedding) WITH (source_field = body, metric = l2, index_type = hnsw, m = 32, ef_construction = 256, ef_search = 256)",
                context.collection, context.collection
            )
        }
        "ivfflat" => {
            context
                .cassie
                .execute_sql(
                    &context.session,
                    &format!(
                        "DROP INDEX {}_embedding_idx ON {}",
                        context.collection, context.collection
                    ),
                    vec![],
                )
                .expect("drop default vector benchmark index");
            format!(
            "CREATE INDEX {}_embedding_ivf_idx ON {} USING vector (embedding) WITH (source_field = body, metric = l2, index_type = ivfflat, lists = 16, probes = 4, training_sample_size = 1024, training_seed = 42)",
            context.collection, context.collection
            )
        }
        _ => unreachable!("benchmark index type is fixed"),
    };
    if !statement.is_empty() {
        context
            .cassie
            .execute_sql(&context.session, &statement, vec![])
            .expect("persisted ANN benchmark index");
    }
    assert_persisted_ann_correctness(runtime, &context, dataset, rows, index_type);

    runner.fixed_timed_count(
        case.metadata("operation_unit", "query"),
        QUERY_BATCH,
        || {
            run_sql_batch(
                runtime,
                &context,
                "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM bench_documents ORDER BY distance ASC LIMIT 20",
                QUERY_BATCH,
            )
        },
    );
}

fn assert_persisted_ann_correctness(
    runtime: &tokio::runtime::Runtime,
    context: &workloads::BenchContext,
    dataset: &str,
    rows: usize,
    index_type: &str,
) {
    let query = "SELECT id, vector_distance(embedding, '[1,0,0]') AS distance FROM bench_documents ORDER BY distance ASC LIMIT 20";
    let before = context.cassie.metrics();
    let indexed = context
        .cassie
        .execute_sql(&context.session, query, vec![])
        .expect("persisted ANN query");
    let exact_context = runtime
        .block_on(workloads::unindexed_context(
            &format!("tier2-vector-exact-{index_type}-{dataset}"),
            rows,
        ))
        .expect("exact ANN benchmark context");
    let exact = exact_context
        .cassie
        .execute_sql(&exact_context.session, query, vec![])
        .expect("exact vector query");

    let exact_ids = exact
        .rows
        .iter()
        .filter_map(|row| match row.first() {
            Some(Value::String(id)) => Some(id.as_str()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let indexed_ids = indexed
        .rows
        .iter()
        .filter_map(|row| match row.first() {
            Some(Value::String(id)) => Some(id.as_str()),
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        indexed_ids.len(),
        20,
        "indexed {index_type} query must return top-k rows"
    );
    assert_eq!(exact_ids.len(), 20, "exact baseline must return top-k rows");
    let overlap = indexed_ids.intersection(&exact_ids).count();
    let minimum_overlap = if index_type == "hnsw" { 14 } else { 12 };
    assert!(
        overlap >= minimum_overlap,
        "{index_type} recall@20 was {overlap}/20 for {dataset}"
    );

    let after = context.cassie.metrics();
    let vector_before = &before["vector"];
    let vector_after = &after["vector"];
    let execution_key = format!("{index_type}_executions");
    let fallback_key = format!("{index_type}_fallbacks");
    assert!(
        vector_after[&execution_key].as_u64().unwrap_or_default()
            > vector_before[&execution_key].as_u64().unwrap_or_default(),
        "{index_type} benchmark query did not execute the persisted index path"
    );
    assert_eq!(
        vector_after[&fallback_key].as_u64().unwrap_or_default(),
        vector_before[&fallback_key].as_u64().unwrap_or_default(),
        "{index_type} benchmark query fell back"
    );
    assert!(
        vector_after["candidate_count_total"]
            .as_u64()
            .unwrap_or_default()
            > vector_before["candidate_count_total"]
                .as_u64()
                .unwrap_or_default(),
        "{index_type} benchmark query did not report candidate reads"
    );
}

struct VectorCandidateFixture {
    query: Vec<f32>,
    records: Vec<NormalizedVectorRecord>,
    brute_force_candidates: Vec<(String, Vec<f32>)>,
    hnsw_options: HnswIndexOptions,
    hnsw_graph: cassie::embeddings::HnswGraphState,
    ivfflat_training: IvfFlatTrainingState,
}

impl VectorCandidateFixture {
    fn new() -> Self {
        let records = vector_records(VECTOR_RECORDS);
        let brute_force_candidates = records
            .iter()
            .map(|record| (record.id.clone(), record.values.clone()))
            .collect::<Vec<_>>();
        let hnsw_options = HnswIndexOptions {
            version: 1,
            m: 16,
            ef_construction: 64,
            ef_search: 40,
        };
        let hnsw_graph = cassie::vector::hnsw::build_graph(
            records.clone(),
            &hnsw_options,
            3,
            DistanceMetric::L2,
        );
        let ivfflat_training = ivfflat_training(&records);

        Self {
            query: vec![0.25, 0.75, 0.5],
            records,
            brute_force_candidates,
            hnsw_options,
            hnsw_graph,
            ivfflat_training,
        }
    }

    fn assert_preflight(&self) {
        assert!(
            cassie::vector::hnsw::graph_fallback_reason(
                Some(&self.hnsw_graph),
                DistanceMetric::L2,
                3,
                &self.records,
            )
            .is_none(),
            "expected compatible HNSW graph"
        );
        assert!(
            cassie::vector::ivfflat::training_compatible(&self.ivfflat_training, 3, &self.records,),
            "expected compatible IVFFlat training"
        );
    }

    fn bruteforce_batch(&self, searches: u64) -> usize {
        let mut selected = 0usize;
        for _ in 0..searches {
            selected = selected.saturating_add(
                cassie::vector::brute_force::top_k(
                    &self.query,
                    self.brute_force_candidates.clone(),
                    20,
                    cassie::vector::l2_distance,
                )
                .len(),
            );
        }
        std::hint::black_box(selected)
    }

    fn hnsw_batch(&self, searches: u64) -> usize {
        let mut selected = 0usize;
        for _ in 0..searches {
            let result = cassie::vector::hnsw::search_graph(
                &self.hnsw_graph,
                &self.query,
                &self.hnsw_options,
                20,
            )
            .expect("HNSW search result");
            selected = selected.saturating_add(result.candidates.len());
        }
        std::hint::black_box(selected)
    }

    fn ivfflat_probe_batch(&self, probes: u64) -> usize {
        let mut selected = 0usize;
        for _ in 0..probes {
            selected = selected.saturating_add(
                cassie::vector::ivfflat::probe_lists(&self.query, &self.ivfflat_training).len(),
            );
        }
        std::hint::black_box(selected)
    }
}

fn vector_records(rows: usize) -> Vec<NormalizedVectorRecord> {
    (0..rows)
        .map(|index| {
            let component = usize_to_f32(index) / usize_to_f32(rows);
            NormalizedVectorRecord {
                collection: "bench_documents".to_string(),
                field: "embedding".to_string(),
                id: format!("doc-{index}"),
                built_generation: 0,
                dimensions: 3,
                metric: DistanceMetric::L2,
                normalization_version: NormalizedVectorRecord::CURRENT_NORMALIZATION_VERSION,
                payload_available: true,
                magnitude: 1.0,
                values: vec![component, 1.0 - component, 0.5],
            }
        })
        .collect()
}

fn ivfflat_training(records: &[NormalizedVectorRecord]) -> IvfFlatTrainingState {
    let mut assignments = BTreeMap::new();
    let mut list_sizes = vec![0usize; IVFFLAT_LISTS];
    for (index, record) in records.iter().enumerate() {
        let list = index % IVFFLAT_LISTS;
        assignments.insert(record.id.clone(), list);
        list_sizes[list] = list_sizes[list].saturating_add(1);
    }
    let centroids = (0..IVFFLAT_LISTS)
        .map(|index| {
            let component = usize_to_f32(index) / usize_to_f32(IVFFLAT_LISTS);
            vec![component, 1.0 - component, 0.5]
        })
        .collect::<Vec<_>>();

    IvfFlatTrainingState {
        version: 1,
        source_fingerprint: cassie::vector::normalized_vector_source_fingerprint(records),
        trained: true,
        row_count: records.len(),
        lists: IVFFLAT_LISTS,
        probes: 4,
        training_seed: 1,
        centroid_ids: (0..IVFFLAT_LISTS)
            .map(|index| format!("centroid-{index}"))
            .collect(),
        centroids,
        assignments,
        list_sizes,
    }
}

fn usize_to_f32(value: usize) -> f32 {
    value
        .to_string()
        .parse::<f32>()
        .expect("benchmark integer should fit f32")
}
