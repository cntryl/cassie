#[path = "support/subsystem.rs"]
mod workloads;

use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_sql_planning",
    );
    let cases = [
        ("sql_binding", stress::OperationUnit::Statement),
        ("logical_planning", stress::OperationUnit::Plan),
        ("physical_planning", stress::OperationUnit::Plan),
    ]
    .map(|(workload, unit)| declared_case(workload, unit));
    let enabled = cases.each_ref().map(|case| runner.is_enabled(case));
    if enabled.into_iter().any(|is_enabled| is_enabled) {
        if enabled[0] {
            let started = Instant::now();
            let fixture = workloads::BindingFixture::new(128);
            runner.measure_counted_batch(
                with_setup(cases[0].clone(), started.elapsed().as_nanos()),
                workloads::PLANNING_FIXTURE_INVOCATIONS_PER_SAMPLE,
                || fixture.bind(),
            );
        }
        if enabled[1] {
            let started = Instant::now();
            let fixture = workloads::LogicalPlanningFixture::new(128);
            runner.measure_counted_batch(
                with_setup(cases[1].clone(), started.elapsed().as_nanos()),
                workloads::PLANNING_FIXTURE_INVOCATIONS_PER_SAMPLE,
                || fixture.logical_plan(),
            );
        }
        if enabled[2] {
            let started = Instant::now();
            let fixture = workloads::PhysicalPlanningFixture::new(128);
            runner.measure_counted_batch(
                with_setup(cases[2].clone(), started.elapsed().as_nanos()),
                workloads::PLANNING_FIXTURE_INVOCATIONS_PER_SAMPLE,
                || fixture.physical_plan(),
            );
        }
    }
    runner.finish();
}

fn with_setup(case: stress::StressCase, setup_time: u128) -> stress::StressCase {
    case.metadata("setup_time_ns", setup_time.to_string())
}

fn declared_case(workload: &str, unit: stress::OperationUnit) -> stress::StressCase {
    stress::StressCase::new(workload, "128").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            128,
            "tier2_subsystem_sql_planning/128",
        ),
        unit,
    )
}
