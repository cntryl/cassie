use std::collections::BTreeMap;
use std::time::Instant;

use cassie_bench_support::stress::{BenchmarkObservation, CountedObservation};
use cassie_bench_support::{performance_benchmarks, stress};
use cassie_vector::types::{
    DistanceMetric, HnswIndexOptions, IvfFlatTrainingState, NormalizedVectorRecord,
};
use cassie_vector::{
    brute_force, hnsw, ivfflat, l2_distance, normalized_vector_source_fingerprint,
};

const ROWS: usize = 1_024;
const BRUTE_FORCE_INVOCATIONS: usize = 512;
const HNSW_INVOCATIONS: usize = 256;
const IVFFLAT_INVOCATIONS: usize = 16_384;
const LISTS: usize = 16;
const QUERY: [f32; 3] = [0.25, 0.75, 0.5];

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_vector",
    );
    let exact = declared_case(
        "vector_bruteforce_candidates",
        stress::OperationUnit::Candidate,
    );
    let hnsw_case = declared_case("vector_hnsw_candidates", stress::OperationUnit::Candidate);
    let ivf = declared_case("vector_ivfflat_probe_lists", stress::OperationUnit::Probe);
    let exact_enabled = runner.is_enabled(&exact);
    let hnsw_enabled = runner.is_enabled(&hnsw_case);
    let ivf_enabled = runner.is_enabled(&ivf);
    if exact_enabled || hnsw_enabled || ivf_enabled {
        let setup_started = Instant::now();
        let fixture = VectorFixture::new(ROWS);
        let setup_time_ns = setup_started.elapsed().as_nanos().to_string();
        if exact_enabled {
            runner.measure_counted(
                with_setup(exact, &setup_time_ns).parameter(
                    "fixture_invocations_per_sample",
                    BRUTE_FORCE_INVOCATIONS.to_string(),
                ),
                || fixture.brute_force_batch(BRUTE_FORCE_INVOCATIONS),
            );
        }
        if hnsw_enabled {
            runner.measure_counted(
                with_setup(hnsw_case, &setup_time_ns).parameter(
                    "fixture_invocations_per_sample",
                    HNSW_INVOCATIONS.to_string(),
                ),
                || fixture.hnsw_batch(HNSW_INVOCATIONS),
            );
        }
        if ivf_enabled {
            runner.measure_counted(
                with_setup(ivf, &setup_time_ns).parameter(
                    "fixture_invocations_per_sample",
                    IVFFLAT_INVOCATIONS.to_string(),
                ),
                || fixture.ivfflat_batch(IVFFLAT_INVOCATIONS),
            );
        }
    }
    runner.finish();
}

fn declared_case(workload: &str, unit: stress::OperationUnit) -> stress::StressCase {
    stress::StressCase::new(workload, "1k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            ROWS,
            "tier2_subsystem_vector/1k",
        ),
        unit,
    )
}

fn with_setup(case: stress::StressCase, setup_time_ns: &str) -> stress::StressCase {
    case.metadata("setup_time_ns", setup_time_ns)
}

struct VectorFixture {
    records: Vec<NormalizedVectorRecord>,
    candidates: Vec<(String, Vec<f32>)>,
    graph: cassie_vector::types::HnswGraphState,
    hnsw_options: HnswIndexOptions,
    training: IvfFlatTrainingState,
}

impl VectorFixture {
    fn new(rows: usize) -> Self {
        let records = (0..rows)
            .map(|index| {
                let component = f32::from(u16::try_from(index).expect("fixture index fits u16"))
                    / f32::from(u16::try_from(rows).expect("row count fits u16"));
                NormalizedVectorRecord {
                    collection: "bench_documents".to_owned(),
                    field: "embedding".to_owned(),
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
            .collect::<Vec<_>>();
        let candidates = records
            .iter()
            .map(|record| (record.id.clone(), record.values.clone()))
            .collect();
        let hnsw_options = HnswIndexOptions::default();
        let graph = hnsw::build_graph(records.clone(), &hnsw_options, 3, DistanceMetric::L2);
        let training = make_training(&records);
        Self {
            records,
            candidates,
            graph,
            hnsw_options,
            training,
        }
    }

    fn brute_force_batch(&self, invocations: usize) -> VectorObservation {
        let mut completed = 0_u64;
        for _ in 0..invocations {
            let selected = brute_force::top_k(&QUERY, self.candidates.clone(), 20, l2_distance);
            assert_eq!(selected.len(), 20);
            std::hint::black_box(selected);
            completed += u64::try_from(self.records.len()).expect("row count fits u64");
        }
        let result_count = invocations
            .checked_mul(20)
            .and_then(|count| u64::try_from(count).ok())
            .expect("exact result count fits u64");
        VectorObservation::new(completed, result_count, Some(completed))
    }

    fn hnsw_batch(&self, invocations: usize) -> VectorObservation {
        let mut completed = 0_u64;
        let mut cardinality = 0_u64;
        let mut candidates = 0_u64;
        for _ in 0..invocations {
            let selected = hnsw::search_graph(&self.graph, &QUERY, &self.hnsw_options, 20)
                .expect("HNSW search should succeed");
            assert_eq!(selected.candidates.len(), 20);
            completed += u64::try_from(selected.candidate_count).expect("candidate count fits u64");
            cardinality += u64::try_from(selected.candidates.len()).expect("result count fits u64");
            candidates +=
                u64::try_from(selected.candidate_count).expect("candidate count fits u64");
            std::hint::black_box(selected);
        }
        VectorObservation::new(completed, cardinality, Some(candidates))
    }

    fn ivfflat_batch(&self, invocations: usize) -> VectorObservation {
        let mut completed = 0_u64;
        for _ in 0..invocations {
            let probes = ivfflat::probe_lists(&QUERY, &self.training);
            assert_eq!(probes.len(), self.training.probes);
            completed += u64::try_from(probes.len()).expect("probe count fits u64");
            std::hint::black_box(probes);
        }
        VectorObservation::new(completed, completed, Some(completed))
    }
}

struct VectorObservation {
    completed_operations: u64,
    result_cardinality: u64,
    candidate_count: Option<u64>,
}

impl VectorObservation {
    const fn new(
        completed_operations: u64,
        result_cardinality: u64,
        candidate_count: Option<u64>,
    ) -> Self {
        Self {
            completed_operations,
            result_cardinality,
            candidate_count,
        }
    }
}

impl BenchmarkObservation for VectorObservation {
    fn cardinality(&self) -> u64 {
        self.result_cardinality
    }

    fn candidate_count(&self) -> Option<u64> {
        self.candidate_count
    }
}

impl CountedObservation for VectorObservation {
    fn completed_operations(&self) -> u64 {
        self.completed_operations
    }

    fn result_cardinality(&self) -> u64 {
        self.result_cardinality
    }

    fn candidate_count(&self) -> Option<u64> {
        self.candidate_count
    }

    fn peak_query_memory_bytes(&self) -> Option<u64> {
        None
    }

    fn finish_sample(self) {}
}

fn make_training(records: &[NormalizedVectorRecord]) -> IvfFlatTrainingState {
    let mut assignments = BTreeMap::new();
    let mut list_sizes = vec![0_usize; LISTS];
    for (index, record) in records.iter().enumerate() {
        let list = index % LISTS;
        assignments.insert(record.id.clone(), list);
        list_sizes[list] += 1;
    }
    let centroids = (0..LISTS)
        .map(|index| {
            let component = f32::from(u16::try_from(index).expect("list index fits u16"))
                / f32::from(u16::try_from(LISTS).expect("list count fits u16"));
            vec![component, 1.0 - component, 0.5]
        })
        .collect::<Vec<_>>();
    IvfFlatTrainingState {
        version: ivfflat::TRAINING_VERSION,
        source_fingerprint: normalized_vector_source_fingerprint(records),
        trained: true,
        row_count: records.len(),
        lists: LISTS,
        probes: 4,
        training_seed: 1,
        centroid_ids: (0..LISTS)
            .map(|index| format!("centroid-{index}"))
            .collect(),
        centroids,
        assignments,
        list_sizes,
        list_bounds: Vec::new(),
    }
}
