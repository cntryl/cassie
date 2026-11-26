#[path = "support/observation.rs"]
mod observation;
#[path = "support/subsystem.rs"]
mod workloads;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_engine::benchmark::SubsystemExecutorKernel;
use observation::ObservationBridge;

const ROWS: usize = 2_048;
const INVOCATIONS: usize = workloads::EXECUTOR_FIXTURE_INVOCATIONS_PER_SAMPLE;
const SUITE: &str = "tier2_subsystem_executor";

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier2, SUITE);
    let cases = [
        ("filter_operator", stress::OperationUnit::Row),
        ("projection_operator", stress::OperationUnit::Row),
        ("top_k_operator", stress::OperationUnit::Candidate),
    ]
    .map(|(workload, unit)| declared_case(workload, unit));
    let enabled: [bool; 3] = std::array::from_fn(|index| runner.is_enabled(&cases[index]));
    if enabled.into_iter().any(|selected| selected) {
        let setup_started = Instant::now();
        let fixture = SubsystemExecutorKernel::with_rows(ROWS);
        let setup_time_ns = setup_started.elapsed().as_nanos().max(1).to_string();
        for (index, selected) in enabled.into_iter().enumerate() {
            if !selected {
                continue;
            }
            let case = cases[index]
                .clone()
                .metadata("setup_time_ns", setup_time_ns)
                .parameter("fixture_invocations_per_sample", INVOCATIONS.to_string());
            runner.measure_counted(case, || {
                let observation = match index {
                    0 => workloads::executor_batch(
                        &fixture,
                        INVOCATIONS,
                        SubsystemExecutorKernel::filter,
                    ),
                    1 => workloads::executor_batch(
                        &fixture,
                        INVOCATIONS,
                        SubsystemExecutorKernel::project,
                    ),
                    _ => workloads::executor_batch(
                        &fixture,
                        INVOCATIONS,
                        SubsystemExecutorKernel::top_k,
                    ),
                };
                ObservationBridge(observation)
            });
        }
    }
    runner.finish();
}

fn declared_case(workload: &str, unit: stress::OperationUnit) -> stress::StressCase {
    stress::StressCase::new(workload, "2k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            ROWS,
            format!("{SUITE}/2k"),
        ),
        unit,
    )
}
use std::time::Instant;
