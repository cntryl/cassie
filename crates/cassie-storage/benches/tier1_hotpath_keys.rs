use cassie_bench_support::{performance_benchmarks, stress};

const KEYS_PER_SAMPLE: u64 = 64;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier1,
        "tier1_hotpath_keys",
    );
    let case = stress::StressCase::new("key_encode_decode", "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            "tier1_hotpath_keys/micro",
        ),
        stress::OperationUnit::Key,
    );
    if runner.is_enabled(&case) {
        runner.measure_micro_batch(case, KEYS_PER_SAMPLE, || {
            for index in 0..KEYS_PER_SAMPLE {
                let key = cassie_storage::adapter::benchmark_row_key_round_trip(
                    7,
                    &format!("doc-{index}"),
                )
                .expect("benchmark key should round trip");
                std::hint::black_box(key);
            }
            KEYS_PER_SAMPLE
        });
    }
    runner.finish();
}
