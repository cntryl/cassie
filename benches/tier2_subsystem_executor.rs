#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_executor",
    );
    let filter = declared_case("filter_operator", stress::OperationUnit::Row);
    let projection = declared_case("projection_operator", stress::OperationUnit::Row);
    let top_k = declared_case("top_k_operator", stress::OperationUnit::Candidate);
    let filter_enabled = runner.is_enabled(&filter);
    let projection_enabled = runner.is_enabled(&projection);
    let top_k_enabled = runner.is_enabled(&top_k);

    if filter_enabled || projection_enabled || top_k_enabled {
        let setup_started = std::time::Instant::now();
        let fixture = workloads::SubsystemExecutorKernel::with_rows(2_048);
        let fixture_invocations = workloads::EXECUTOR_FIXTURE_INVOCATIONS_PER_SAMPLE;
        let setup_time_ns = setup_started.elapsed().as_nanos().to_string();
        if filter_enabled {
            runner.measure_counted(
                with_setup(filter, &setup_time_ns).parameter(
                    "fixture_invocations_per_sample",
                    fixture_invocations.to_string(),
                ),
                || workloads::executor_filter_batch(&fixture, fixture_invocations),
            );
        }
        if projection_enabled {
            runner.measure_counted(
                with_setup(projection, &setup_time_ns).parameter(
                    "fixture_invocations_per_sample",
                    fixture_invocations.to_string(),
                ),
                || workloads::executor_projection_batch(&fixture, fixture_invocations),
            );
        }
        if top_k_enabled {
            runner.measure_counted(
                with_setup(top_k, &setup_time_ns).parameter(
                    "fixture_invocations_per_sample",
                    fixture_invocations.to_string(),
                ),
                || workloads::executor_top_k_batch(&fixture, fixture_invocations),
            );
        }
    }
    runner.finish();
}

fn with_setup(case: stress::StressCase, setup_time_ns: &str) -> stress::StressCase {
    case.metadata("setup_time_ns", setup_time_ns)
}

fn declared_case(workload: &str, operation_unit: stress::OperationUnit) -> stress::StressCase {
    stress::StressCase::new(workload, "2k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            2_048,
            "tier2_subsystem_executor/2k",
        ),
        operation_unit,
    )
}
