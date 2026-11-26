#[path = "support/subsystem.rs"]
mod workloads;

use std::time::Instant;

use cassie_bench_support::{performance_benchmarks, stress};

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_binder",
    );
    let cases = [
        ("sql_binder", stress::OperationUnit::Statement),
        ("parameter_binding", stress::OperationUnit::Parameter),
        ("logical_planner", stress::OperationUnit::Plan),
        ("physical_planner", stress::OperationUnit::Plan),
    ]
    .map(|(workload, unit)| declared_case(workload, unit));
    let enabled = cases.map(|case| runner.is_enabled(&case));
    if enabled.into_iter().any(|is_enabled| is_enabled) {
        if enabled[0] {
            let start = Instant::now();
            let fixture = workloads::BindingFixture::new(128);
            runner.measure_counted_batch(
                with_setup(cases[0].clone(), start.elapsed().as_nanos()),
                workloads::PLANNING_FIXTURE_INVOCATIONS_PER_SAMPLE,
                || fixture.bind(),
            );
        }
        if enabled[1] {
            let start = Instant::now();
            let fixture = workloads::ParameterBindingFixture::new(128);
            runner.measure_counted_batch(
                with_setup(cases[1].clone(), start.elapsed().as_nanos()),
                workloads::PARAMETER_BINDING_INVOCATIONS_PER_SAMPLE,
                || fixture.bind_parameters(),
            );
        }
        if enabled[2] {
            let start = Instant::now();
            let fixture = workloads::LogicalPlanningFixture::new(128);
            runner.measure_counted_batch(
                with_setup(cases[2].clone(), start.elapsed().as_nanos()),
                workloads::PLANNING_FIXTURE_INVOCATIONS_PER_SAMPLE,
                || fixture.logical_plan(),
            );
        }
        if enabled[3] {
            let start = Instant::now();
            let fixture = workloads::PhysicalPlanningFixture::new(128);
            runner.measure_counted_batch(
                with_setup(cases[3].clone(), start.elapsed().as_nanos()),
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
            "tier2_subsystem_binder/128",
        ),
        unit,
    )
}
