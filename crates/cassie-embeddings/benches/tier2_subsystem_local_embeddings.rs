use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_embeddings::local::{LocalProvider, LocalProviderConfig};
use cassie_embeddings::EmbeddingProvider;

const DIMENSIONS: usize = 768;
const DOCUMENTS: usize = 32;
const INPUTS: [&str; DOCUMENTS] = [
    "projection event one",
    "projection event two",
    "projection event three",
    "projection event four",
    "projection event five",
    "projection event six",
    "projection event seven",
    "projection event eight",
    "projection event nine",
    "projection event ten",
    "projection event eleven",
    "projection event twelve",
    "projection event thirteen",
    "projection event fourteen",
    "projection event fifteen",
    "projection event sixteen",
    "projection event seventeen",
    "projection event eighteen",
    "projection event nineteen",
    "projection event twenty",
    "projection event twenty one",
    "projection event twenty two",
    "projection event twenty three",
    "projection event twenty four",
    "projection event twenty five",
    "projection event twenty six",
    "projection event twenty seven",
    "projection event twenty eight",
    "projection event twenty nine",
    "projection event thirty",
    "projection event thirty one",
    "projection event thirty two",
];

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_local_embeddings",
    );
    let case = stress::StressCase::new("local_document_embedding_batch", "32x768")
        .parameter("documents_per_batch", DOCUMENTS.to_string())
        .parameter("dimensions", DIMENSIONS.to_string())
        .runtime_contract(
            stress::FixtureDeclaration::new(
                performance_benchmarks::FixtureClass::Subsystem,
                DOCUMENTS,
                "tier2_subsystem_local_embeddings/32x768",
            ),
            stress::OperationUnit::Document,
        );
    if runner.is_enabled(&case) {
        let setup_started = Instant::now();
        let provider = provider();
        let inputs = INPUTS.iter().map(ToString::to_string).collect::<Vec<_>>();
        runner.measure_counted(
            case.metadata(
                "setup_time_ns",
                setup_started.elapsed().as_nanos().max(1).to_string(),
            ),
            || {
                let embeddings = provider
                    .embed_documents(&inputs)
                    .expect("local document embeddings should succeed");
                assert_eq!(embeddings.len(), DOCUMENTS);
                assert!(embeddings
                    .iter()
                    .all(|embedding| embedding.values.len() == DIMENSIONS));
                std::hint::black_box(embeddings);
                u64::try_from(DOCUMENTS).expect("document count fits u64")
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
