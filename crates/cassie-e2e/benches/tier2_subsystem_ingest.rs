#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_ingest",
    );
    let write = stress::StressCase::new("projection_write_batch", "2k");
    let replay = stress::StressCase::new("projection_replay_batch", "2k");
    let write_enabled = runner.is_enabled(&write);
    let replay_enabled = runner.is_enabled(&replay);

    if write_enabled || replay_enabled {
        let setup_started = std::time::Instant::now();
        let runtime = workloads::runtime();
        let fixture = workloads::ProjectionBatchFixture::new(&runtime, 2_048);
        let setup_time_ns = setup_started.elapsed().as_nanos().to_string();
        let fixture_identity = fixture.fixture_identity().to_string();
        if write_enabled {
            let write = declared_case(
                "projection_write_batch",
                stress::OperationUnit::Document,
                &fixture_identity,
            );
            let case = with_setup(write, &setup_time_ns).runtime_evidence(fixture.cassie());
            runner.measure_counted(case, || fixture.write_batch());
        }
        if replay_enabled {
            let replay = declared_case(
                "projection_replay_batch",
                stress::OperationUnit::Event,
                &fixture_identity,
            );
            let case = with_setup(replay, &setup_time_ns).runtime_evidence(fixture.cassie());
            runner.measure_counted(case, || fixture.replay_batch());
        }
    }
    runner.finish();
}

fn with_setup(case: stress::StressCase, setup_time_ns: &str) -> stress::StressCase {
    case.metadata("setup_time_ns", setup_time_ns)
}

fn declared_case(
    workload: &str,
    operation_unit: stress::OperationUnit,
    fixture_identity: &str,
) -> stress::StressCase {
    stress::StressCase::new(workload, "2k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            2_048,
            fixture_identity,
        ),
        operation_unit,
    )
}
