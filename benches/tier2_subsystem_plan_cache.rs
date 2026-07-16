#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_plan_cache",
    );
    let plan_hit = declared_case("plan_cache_hit");
    let result_hit = declared_case("execution_result_cache_hit");
    let plan_miss = declared_case("plan_cache_miss");
    let plan_hit_enabled = runner.is_enabled(&plan_hit);
    let result_hit_enabled = runner.is_enabled(&result_hit);
    let plan_miss_enabled = runner.is_enabled(&plan_miss);

    if plan_hit_enabled || result_hit_enabled || plan_miss_enabled {
        let setup_started = std::time::Instant::now();
        let fixture = workloads::CacheFixture::new(1_024, result_hit_enabled);
        let setup_time_ns = setup_started.elapsed().as_nanos().to_string();
        if plan_hit_enabled {
            let case =
                with_setup(plan_hit, &setup_time_ns).runtime_state_evidence(fixture.plan_runtime());
            runner.measure(case, || fixture.plan_hit());
        }
        if result_hit_enabled {
            let case = with_setup(result_hit, &setup_time_ns)
                .runtime_state_evidence(fixture.result_runtime());
            runner.measure(case, || fixture.result_hit());
        }
        if plan_miss_enabled {
            let case = with_setup(plan_miss, &setup_time_ns)
                .runtime_state_evidence(fixture.plan_runtime());
            runner.measure(case, || fixture.plan_miss());
        }
    }
    runner.finish();
}

fn with_setup(case: stress::StressCase, setup_time_ns: &str) -> stress::StressCase {
    case.metadata("setup_time_ns", setup_time_ns)
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
