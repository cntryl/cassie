#[path = "support/observation.rs"]
mod observation;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_engine::benchmark::ExecutorKernel;
use observation::ObservationBridge;
use std::time::Instant;

const OPERATIONS_PER_SAMPLE: u64 = 32;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier1,
        "tier1_hotpath_topk",
    );
    let case = stress::StressCase::new("top_k_heap_maintenance", "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            "tier1_hotpath_topk/micro",
        ),
        stress::OperationUnit::TopKMaintenance,
    );
    if runner.is_enabled(&case) {
        let setup_started = Instant::now();
        let kernel = ExecutorKernel::sample();
        let setup_time_ns = setup_started.elapsed().as_nanos().max(1).to_string();
        runner.measure_micro_batch(
            case.metadata("setup_time_ns", setup_time_ns),
            OPERATIONS_PER_SAMPLE,
            || {
                let mut rows = 0_u64;
                let mut candidates = 0_u64;
                for _ in 0..OPERATIONS_PER_SAMPLE {
                    rows = rows.saturating_add(
                        u64::try_from(kernel.top_k_scores().len()).expect("top-k rows fit"),
                    );
                    candidates = candidates.saturating_add(
                        u64::try_from(kernel.top_k_candidate_count())
                            .expect("top-k candidates fit"),
                    );
                }
                ObservationBridge(
                    cassie_engine::benchmark::KernelObservation::new(OPERATIONS_PER_SAMPLE, rows)
                        .with_candidate_count(candidates),
                )
            },
        );
    }
    runner.finish();
}
