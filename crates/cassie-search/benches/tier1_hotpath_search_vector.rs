use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_search::tokenize;

const TOKENIZATION_BATCH_SIZE: u64 = 256;
const TOKENIZATION_INPUTS: [&str; 8] = [
    "Alpha beta, gamma and delta",
    "Cassie plans selective indexed queries",
    "Midge persists ordered document keys",
    "Hybrid retrieval combines text and vectors",
    "PostgreSQL clients reuse prepared statements",
    "Column batches accelerate analytical filters",
    "Snapshots preserve catalog and row state",
    "Operators inspect bounded runtime metrics",
];

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier1,
        "tier1_hotpath_search_vector",
    );
    let case = stress::StressCase::new("tokenization", "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            "tier1_hotpath_search_vector/micro",
        ),
        stress::OperationUnit::Text,
    );
    if runner.is_enabled(&case) {
        let setup_started = Instant::now();
        runner.measure_micro_batch(
            case.metadata(
                "setup_time_ns",
                setup_started.elapsed().as_nanos().max(1).to_string(),
            ),
            TOKENIZATION_BATCH_SIZE,
            tokenization_batch,
        );
    }
    runner.finish();
}

fn tokenization_batch() -> u64 {
    let mut observed_tokens = 0_usize;
    for position in 0..TOKENIZATION_BATCH_SIZE {
        let position = usize::try_from(position).expect("batch position fits usize");
        observed_tokens = observed_tokens.saturating_add(
            tokenize(std::hint::black_box(
                TOKENIZATION_INPUTS[position % TOKENIZATION_INPUTS.len()],
            ))
            .len(),
        );
    }
    std::hint::black_box(observed_tokens);
    TOKENIZATION_BATCH_SIZE
}
