#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner(
        performance_benchmarks::BenchmarkTier::Tier2,
        "tier2_subsystem_vector",
    );
    let brute_force = declared_case(
        "vector_bruteforce_candidates",
        stress::OperationUnit::Candidate,
    );
    let hnsw = declared_case("vector_hnsw_candidates", stress::OperationUnit::Candidate);
    let ivfflat = declared_case("vector_ivfflat_probe_lists", stress::OperationUnit::Probe);
    let brute_force_enabled = runner.is_enabled(&brute_force);
    let hnsw_enabled = runner.is_enabled(&hnsw);
    let ivfflat_enabled = runner.is_enabled(&ivfflat);

    if brute_force_enabled || hnsw_enabled || ivfflat_enabled {
        let setup_started = std::time::Instant::now();
        let fixture = workloads::VectorCandidateFixture::new(1_024);
        let setup_time_ns = setup_started.elapsed().as_nanos().to_string();
        if brute_force_enabled {
            let case = with_setup(brute_force, &setup_time_ns).parameter(
                "fixture_invocations_per_sample",
                workloads::VECTOR_BRUTE_FORCE_INVOCATIONS_PER_SAMPLE.to_string(),
            );
            runner.measure_counted(case, || {
                fixture.brute_force_batch(workloads::VECTOR_BRUTE_FORCE_INVOCATIONS_PER_SAMPLE)
            });
        }
        if hnsw_enabled {
            let case = with_setup(hnsw, &setup_time_ns).parameter(
                "fixture_invocations_per_sample",
                workloads::VECTOR_HNSW_INVOCATIONS_PER_SAMPLE.to_string(),
            );
            runner.measure_counted(case, || {
                fixture.hnsw_batch(workloads::VECTOR_HNSW_INVOCATIONS_PER_SAMPLE)
            });
        }
        if ivfflat_enabled {
            let case = with_setup(ivfflat, &setup_time_ns).parameter(
                "fixture_invocations_per_sample",
                workloads::VECTOR_IVFFLAT_INVOCATIONS_PER_SAMPLE.to_string(),
            );
            runner.measure_counted(case, || {
                fixture.ivfflat_batch(workloads::VECTOR_IVFFLAT_INVOCATIONS_PER_SAMPLE)
            });
        }
    }
    runner.finish();
}

fn with_setup(case: stress::StressCase, setup_time_ns: &str) -> stress::StressCase {
    case.metadata("setup_time_ns", setup_time_ns)
}

fn declared_case(workload: &str, operation_unit: stress::OperationUnit) -> stress::StressCase {
    stress::StressCase::new(workload, "1k").runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Subsystem,
            1_024,
            "tier2_subsystem_vector/1k",
        ),
        operation_unit,
    )
}
