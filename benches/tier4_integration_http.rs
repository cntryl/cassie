const BENCHMARK: &str = "tier4_integration_http";
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
    let document = declared_case("document_create_get");
    let vector = declared_case("vector_search");
    let query = declared_case("query");
    let enabled = [
        runner.is_enabled(&document),
        runner.is_enabled(&vector),
        runner.is_enabled(&query),
    ];
    if !enabled.iter().any(|selected| *selected) {
        runner.finish();
        return;
    }

    let setup_started = std::time::Instant::now();
    let generated_http_tls =
        workloads::configure_http_tls().expect("configure benchmark REST TLS identity");
    let runtime = workloads::runtime();
    let fixture = runtime
        .block_on(workloads::context_with_mock_tei_embeddings(
            "tier4-http-10k",
            FIXTURE_ROWS,
            FIXTURE_ROWS,
        ))
        .expect("Tier 4 HTTP fixture");
    let context = runtime
        .block_on(workloads::http_transport_context(&fixture))
        .expect("Tier 4 HTTP transport context");
    let setup_time_ns = setup_started.elapsed().as_nanos().to_string();

    if enabled[0] {
        runner.record_external(
            evidenced(document, &setup_time_ns, 1, &fixture),
            |sample_duration| {
                transport_external::sample_until_deadline(sample_duration, || {
                    u64::try_from(
                        runtime.block_on(workloads::http_transport_document_create_get(&context)),
                    )
                    .expect("HTTP document request count should fit u64")
                })
            },
        );
    }
    if enabled[1] {
        runner.record_external(
            evidenced(vector, &setup_time_ns, 10, &fixture),
            |sample_duration| {
                transport_external::sample_until_deadline(sample_duration, || {
                    let rows = runtime.block_on(workloads::http_transport_vector_search(&context));
                    assert_eq!(rows, 10, "HTTP vector result cardinality");
                    1
                })
            },
        );
    }
    if enabled[2] {
        runner.record_external(
            evidenced(query, &setup_time_ns, 20, &fixture),
            |sample_duration| {
                transport_external::sample_until_deadline(sample_duration, || {
                    u64::try_from(runtime.block_on(workloads::http_transport_query(&context)))
                        .expect("HTTP query request count should fit u64")
                })
            },
        );
    }

    runtime
        .block_on(context.shutdown())
        .expect("graceful HTTP benchmark shutdown");
    if let Some(material) = generated_http_tls {
        material
            .cleanup()
            .expect("clean up generated REST TLS identity");
    }
    runner.finish();
}

fn declared_case(workload: &str) -> stress::StressCase {
    stress::StressCase::new(workload, FIXTURE_SCALE).runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Integration,
            FIXTURE_ROWS,
            "tier4_integration_http/10k",
        ),
        stress::OperationUnit::Request,
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
