use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_embeddings::local::{LocalProvider, LocalProviderConfig};
use cassie_embeddings::EmbeddingProvider;

const DIMENSIONS: usize = 768;
const BATCH_SIZE: u64 = 32;
const INPUTS: [&str; 8] = [
    "event sourced read models",
    "durable projection updates",
    "postgres wire query planning",
    "deterministic local embeddings",
    "bounded request memory",
    "hybrid text vector search",
    "single node database",
    "checkpoint recovery evidence",
];

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier1,
        "tier1_hotpath_local_embedding",
    );
    let case = stress::StressCase::new("local_query_embedding", "768-dimensions")
        .parameter("dimensions", DIMENSIONS.to_string())
        .runtime_contract(
            stress::FixtureDeclaration::new(
                performance_benchmarks::FixtureClass::Kernel,
                0,
                "tier1_hotpath_local_embedding/768-dimensions",
            ),
            stress::OperationUnit::Document,
        );
    if runner.is_enabled(&case) {
        let setup_started = Instant::now();
        let provider = provider();
        runner.measure_micro_batch(
            case.metadata(
                "setup_time_ns",
                setup_started.elapsed().as_nanos().max(1).to_string(),
            ),
            BATCH_SIZE,
            || {
                let mut dimensions = 0_usize;
                for position in 0..BATCH_SIZE {
                    let position = usize::try_from(position).expect("position fits usize");
                    let embedding = provider
                        .embed_query(INPUTS[position % INPUTS.len()])
                        .expect("local query embedding should succeed");
                    assert_eq!(embedding.values.len(), DIMENSIONS);
                    dimensions += embedding.values.len();
                    std::hint::black_box(embedding);
                }
                std::hint::black_box(dimensions);
                BATCH_SIZE
            },
        );
    }
    runner.finish();
}

fn provider() -> LocalProvider {
    LocalProvider::with_config(LocalProviderConfig {
        model: "cassie-benchmark-local-v1".to_owned(),
        dimensions: DIMENSIONS,
    })
    .expect("benchmark local provider config")
}
