#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_sql_planning",
    );
    let parsing = declared_case("sql_parsing", stress::OperationUnit::Statement);
    let binding = declared_case("sql_binding", stress::OperationUnit::Statement);
    let logical = declared_case("logical_planning", stress::OperationUnit::Plan);
    let physical = declared_case("physical_planning", stress::OperationUnit::Plan);
    let parsing_enabled = runner.is_enabled(&parsing);
    let binding_enabled = runner.is_enabled(&binding);
    let logical_enabled = runner.is_enabled(&logical);
    let physical_enabled = runner.is_enabled(&physical);

    if parsing_enabled {
        let setup_started = std::time::Instant::now();
        let fixture = workloads::ParserFixture::new(128);
        let setup_time_ns = setup_started.elapsed().as_nanos().to_string();
        runner.measure_counted(with_setup(parsing, &setup_time_ns), || fixture.parse());
    }
    if binding_enabled {
        let setup_started = std::time::Instant::now();
        let fixture = workloads::BindingFixture::new(128);
        let setup_time_ns = setup_started.elapsed().as_nanos().to_string();
        runner.measure_counted(with_setup(binding, &setup_time_ns), || fixture.bind());
    }
    if logical_enabled {
        let setup_started = std::time::Instant::now();
        let fixture = workloads::LogicalPlanningFixture::new(128);
        let setup_time_ns = setup_started.elapsed().as_nanos().to_string();
        runner.measure_counted(with_setup(logical, &setup_time_ns), || {
            fixture.logical_plan()
        });
    }
    if physical_enabled {
        let setup_started = std::time::Instant::now();
        let fixture = workloads::PhysicalPlanningFixture::new(128);
        let setup_time_ns = setup_started.elapsed().as_nanos().to_string();
        runner.measure_counted(with_setup(physical, &setup_time_ns), || {
            fixture.physical_plan()
        });
    }
    runner.finish();
}

fn with_setup(case: stress::StressCase, setup_time_ns: &str) -> stress::StressCase {
    case.metadata("setup_time_ns", setup_time_ns)
}

fn declared_case(workload: &str, operation_unit: stress::OperationUnit) -> stress::StressCase {
    stress::StressCase::new(workload, "128").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            128,
            "tier2_subsystem_sql_planning/128",
        ),
        operation_unit,
    )
}
