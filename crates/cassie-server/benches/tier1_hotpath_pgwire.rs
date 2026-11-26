use std::time::Instant;

use cassie::benchmark::PgwireRowCodecKernel;
use cassie_bench_support::{performance_benchmarks, stress};

const ROWS_PER_SAMPLE: u64 = 64;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier1,
        "tier1_hotpath_pgwire",
    );
    let case = stress::StressCase::new("row_to_pgwire_encoding", "micro").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Kernel,
            0,
            "tier1_hotpath_pgwire/micro",
        ),
        stress::OperationUnit::Row,
    );
    if runner.is_enabled(&case) {
        let setup_started = Instant::now();
        let samples = (0..ROWS_PER_SAMPLE)
            .map(|index| {
                PgwireRowCodecKernel::sample(
                    usize::try_from(index).expect("row fixture index fits usize"),
                )
            })
            .collect::<Vec<_>>();
        let case = case.metadata(
            "setup_time_ns",
            setup_started.elapsed().as_nanos().max(1).to_string(),
        );
        runner.measure_micro_batch(case, ROWS_PER_SAMPLE, || {
            let encoded_bytes = samples
                .iter()
                .map(PgwireRowCodecKernel::encode)
                .map(|bytes| std::hint::black_box(bytes.len()))
                .sum::<usize>();
            std::hint::black_box(encoded_bytes);
            ROWS_PER_SAMPLE
        });
    }
    runner.finish();
}
