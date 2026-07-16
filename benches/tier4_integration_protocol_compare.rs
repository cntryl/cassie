const BENCHMARK: &str = "tier4_integration_protocol_compare";
const FIXTURE_SCALE: &str = "10k";
const FIXTURE_ROWS: usize = 10_000;

#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/transport_external.rs"]
mod transport_external;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier4, BENCHMARK);
    let pgwire_case = declared_case("pgwire_query");
    let http_case = declared_case("http_query");
    let pgwire_enabled = runner.is_enabled(&pgwire_case);
    let http_enabled = runner.is_enabled(&http_case);
    if !pgwire_enabled && !http_enabled {
        runner.finish();
        return;
    }

    let setup_started = std::time::Instant::now();
    let generated_http_tls = configure_generated_http_tls(http_enabled);
    let runtime = workloads::runtime();
    let fixture = runtime
        .block_on(workloads::context(
            "tier4-protocol-compare-10k",
            FIXTURE_ROWS,
        ))
        .expect("Tier 4 protocol comparison fixture");
    let pgwire_preflight = pgwire_enabled.then(|| {
        workloads::assert_explain_contains(
            &fixture,
            workloads::PGWIRE_SIMPLE_QUERY,
            vec![],
            "access_path=collection_scan",
        )
    });
    let http_preflight = http_enabled.then(|| {
        workloads::assert_explain_contains(
            &fixture,
            workloads::HTTP_ADMIN_QUERY,
            vec![],
            "access_path=collection_scan",
        )
    });
    let pgwire = pgwire_enabled.then(|| {
        runtime
            .block_on(workloads::pgwire_transport_for_context(&fixture))
            .expect("protocol comparison pgwire context")
    });
    let http = http_enabled.then(|| {
        runtime
            .block_on(workloads::http_transport_context(&fixture))
            .expect("protocol comparison HTTP context")
    });
    let setup_time_ns = setup_started.elapsed().as_nanos().to_string();

    if pgwire_enabled {
        let context = pgwire.as_ref().expect("enabled pgwire context");
        runner.record_external(
            query_evidenced(
                pgwire_case,
                &setup_time_ns,
                20,
                &fixture,
                pgwire_preflight.expect("enabled comparison pgwire preflight"),
            ),
            |sample_duration| {
                transport_external::sample_until_deadline(sample_duration, || {
                    let rows = runtime.block_on(workloads::pgwire_transport_simple_query(
                        context,
                        workloads::PGWIRE_SIMPLE_QUERY,
                    ));
                    assert_eq!(rows, 20, "comparison pgwire result cardinality");
                    1
                })
            },
        );
    }
    if http_enabled {
        let context = http.as_ref().expect("enabled HTTP context");
        runner.record_external(
            query_evidenced(
                http_case,
                &setup_time_ns,
                20,
                &fixture,
                http_preflight.expect("enabled comparison HTTP preflight"),
            ),
            |sample_duration| {
                transport_external::sample_until_deadline(sample_duration, || {
                    u64::try_from(runtime.block_on(workloads::http_transport_query(context)))
                        .expect("comparison HTTP query count should fit u64")
                })
            },
        );
    }

    if let Some(context) = pgwire {
        runtime
            .block_on(context.shutdown())
            .expect("graceful comparison pgwire shutdown");
    }
    if let Some(context) = http {
        runtime
            .block_on(context.shutdown())
            .expect("graceful comparison HTTP shutdown");
    }
    cleanup_generated_http_tls(generated_http_tls);
    runner.finish();
}

fn configure_generated_http_tls(enabled: bool) -> Option<workloads::GeneratedHttpTlsMaterial> {
    if enabled {
        workloads::configure_http_tls().expect("configure benchmark REST TLS identity")
    } else {
        None
    }
}

fn cleanup_generated_http_tls(material: Option<workloads::GeneratedHttpTlsMaterial>) {
    if let Some(material) = material {
        material
            .cleanup()
            .expect("clean up generated REST TLS identity");
    }
}

fn declared_case(workload: &str) -> stress::StressCase {
    stress::StressCase::new(workload, FIXTURE_SCALE).runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Integration,
            FIXTURE_ROWS,
            "tier4_integration_protocol_compare/10k",
        ),
        stress::OperationUnit::Query,
    )
}

fn evidenced(
    case: stress::StressCase,
    setup_time_ns: &str,
    result_cardinality: u64,
    fixture: &workloads::BenchContext,
) -> stress::StressCase {
    case.metadata("setup_time_ns", setup_time_ns)
        .metadata("result_cardinality", result_cardinality.to_string())
        .runtime_evidence(fixture.cassie.clone())
}

fn query_evidenced(
    case: stress::StressCase,
    setup_time_ns: &str,
    result_cardinality: u64,
    fixture: &workloads::BenchContext,
    preflight: workloads::QueryPreflightEvidence,
) -> stress::StressCase {
    evidenced(case, setup_time_ns, result_cardinality, fixture)
        .preflight_evidence(preflight.selected_access_path, preflight.fallback_reason)
}
