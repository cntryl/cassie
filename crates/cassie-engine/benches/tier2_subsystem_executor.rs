use std::time::Instant;

#[path = "support/observation.rs"]
mod observation;

use cassie_bench_support::{performance_benchmarks, stress};
use cassie_engine::benchmark::{KernelObservation, SubsystemExecutorKernel};
use observation::ObservationBridge;

const ROWS: usize = 2_048;
const INVOCATIONS: usize = 8;
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
                .metadata("setup_time_ns", &setup_time_ns)
                .parameter("fixture_invocations_per_sample", INVOCATIONS.to_string());
            runner.measure_counted(case, || {
                let observation = match index {
                    0 => executor_batch(&fixture, INVOCATIONS, SubsystemExecutorKernel::filter),
                    1 => executor_batch(&fixture, INVOCATIONS, SubsystemExecutorKernel::project),
                    _ => executor_batch(&fixture, INVOCATIONS, SubsystemExecutorKernel::top_k),
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

fn executor_batch(
    fixture: &SubsystemExecutorKernel,
    invocations: usize,
    mut operation: impl FnMut(&SubsystemExecutorKernel) -> KernelObservation,
) -> KernelObservation {
    assert!(invocations > 0, "executor batch must not be empty");
    let mut completed = 0_u64;
    let mut cardinality = 0_u64;
    let mut candidates = 0_u64;
    let mut saw_candidates = false;
    for _ in 0..invocations {
        let observation = operation(fixture);
        completed = completed
            .checked_add(observation.completed_operations())
            .expect("executor batch operation count fits u64");
        cardinality = cardinality
            .checked_add(observation.result_cardinality())
            .expect("executor batch result count fits u64");
        if let Some(count) = observation.candidate_count() {
            candidates = candidates
                .checked_add(count)
                .expect("executor batch candidate count fits u64");
            saw_candidates = true;
        }
        observation.finish_sample();
    }
    let batch = KernelObservation::new(completed, cardinality);
    if saw_candidates {
        batch.with_candidate_count(candidates)
    } else {
        batch
    }
}
