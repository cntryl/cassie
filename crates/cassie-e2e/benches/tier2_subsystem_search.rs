#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

const POSTING_MERGE_INVOCATIONS_PER_SAMPLE: usize = 512;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_search",
    );
    let case = stress::StressCase::new("posting_merge", "2k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            2_048,
            "tier2_subsystem_search/2k",
        ),
        stress::OperationUnit::Candidate,
    );
    if runner.is_enabled(&case) {
        let setup_started = std::time::Instant::now();
        let fixture = workloads::PostingMergeFixture::new(2_048);
        let case = case.parameter(
            "fixture_invocations_per_sample",
            POSTING_MERGE_INVOCATIONS_PER_SAMPLE.to_string(),
        );
        runner.measure_counted(
            case.metadata(
                "setup_time_ns",
                setup_started.elapsed().as_nanos().to_string(),
            ),
            || fixture.merge_batch(POSTING_MERGE_INVOCATIONS_PER_SAMPLE),
        );
    }
    runner.finish();
}
