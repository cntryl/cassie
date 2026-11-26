#[path = "support/subsystem_cache.rs"]
mod cache;

use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};

const LOOKUPS_PER_SAMPLE: usize = 8_192;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_plan_cache",
    );
    let cases = [
        "plan_cache_hit",
        "execution_result_cache_hit",
        "plan_cache_miss",
    ]
    .map(declared_case);
    let enabled: [bool; 3] = std::array::from_fn(|index| runner.is_enabled(&cases[index]));
    if enabled.into_iter().any(|is_enabled| is_enabled) {
        let setup_started = Instant::now();
        let fixture = cache::CacheFixture::new(1_024, enabled[1]);
        let setup_time_ns = setup_started.elapsed().as_nanos().max(1).to_string();
        if enabled[0] {
            let runtime = fixture.plan_runtime();
            let case = cases[0]
                .clone()
                .runtime_evidence(move || {
                    serde_json::to_value(runtime.snapshot()).expect("serialize plan cache snapshot")
                })
                .metadata("setup_time_ns", &setup_time_ns);
            runner.measure_counted_batch(case, LOOKUPS_PER_SAMPLE, || {
                u64::try_from(fixture.plan_hit()).expect("plan hit count fits u64")
            });
        }
        if enabled[1] {
            let runtime = fixture.result_runtime();
            let case = cases[1]
                .clone()
                .runtime_evidence(move || {
                    serde_json::to_value(runtime.snapshot())
                        .expect("serialize result cache snapshot")
                })
                .metadata("setup_time_ns", &setup_time_ns);
            runner.measure_counted_batch(case, LOOKUPS_PER_SAMPLE, || {
                u64::try_from(fixture.result_hit()).expect("result hit count fits u64")
            });
        }
        if enabled[2] {
            let runtime = fixture.plan_runtime();
            let case = cases[2]
                .clone()
                .runtime_evidence(move || {
                    serde_json::to_value(runtime.snapshot()).expect("serialize plan cache snapshot")
                })
                .metadata("setup_time_ns", &setup_time_ns);
            runner.measure_counted_batch(case, LOOKUPS_PER_SAMPLE, || {
                u64::try_from(fixture.plan_miss()).expect("plan miss count fits u64")
            });
        }
    }
    runner.finish();
}

fn declared_case(workload: &str) -> stress::StressCase {
    stress::StressCase::new(workload, "1k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            1_024,
            "tier2_subsystem_plan_cache/1k",
        ),
        stress::OperationUnit::Lookup,
    )
}
