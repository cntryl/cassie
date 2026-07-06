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

#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let mut runner = stress::runner(BENCHMARK);

    for (dataset, rows) in [("10k", 10_000), ("100k", 100_000)] {
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
