#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_protocol_handlers",
    );
    let pgwire = declared_case("pgwire_codec", stress::OperationUnit::Message);
    let prepared = declared_case("prepared_statement_loop", stress::OperationUnit::Message);
    let json = declared_case("json_serialization", stress::OperationUnit::Row);
    let pgwire_enabled = runner.is_enabled(&pgwire);
    let prepared_enabled = runner.is_enabled(&prepared);
    let json_enabled = runner.is_enabled(&json);

    if pgwire_enabled || prepared_enabled || json_enabled {
        let setup_started = std::time::Instant::now();
        let fixture = workloads::ProtocolCodecFixture::new(512);
        let setup_time_ns = setup_started.elapsed().as_nanos().to_string();
        if pgwire_enabled {
            runner.measure_counted(with_setup(pgwire, &setup_time_ns), || {
                fixture.pgwire_codec()
            });
        }
        if prepared_enabled {
            runner.measure_counted(with_setup(prepared, &setup_time_ns), || {
                fixture.prepared_loop()
            });
        }
        if json_enabled {
            runner.measure_counted(with_setup(json, &setup_time_ns), || {
                fixture.json_serialization()
            });
        }
    }
    runner.finish();
}

fn with_setup(case: stress::StressCase, setup_time_ns: &str) -> stress::StressCase {
    case.metadata("setup_time_ns", setup_time_ns)
}

fn declared_case(workload: &str, operation_unit: stress::OperationUnit) -> stress::StressCase {
    stress::StressCase::new(workload, "512").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            512,
            "tier2_subsystem_protocol_handlers/512",
        ),
        operation_unit,
    )
}
