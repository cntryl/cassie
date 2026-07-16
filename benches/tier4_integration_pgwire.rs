const BENCHMARK: &str = "tier4_integration_pgwire";
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
    let simple = declared_case("simple_query", stress::OperationUnit::Query);
    let extended = declared_case("extended_query", stress::OperationUnit::Query);
    let portal = declared_case("portal_fetch", stress::OperationUnit::Fetch);
    let cancellation = declared_case("cancellation", stress::OperationUnit::Cancel);
    let multi_statement = declared_case("multi_statement", stress::OperationUnit::Query);
    let binary_extended = declared_case("binary_extended_query", stress::OperationUnit::Query);
    let enabled = [
        runner.is_enabled(&simple),
        runner.is_enabled(&extended),
        runner.is_enabled(&portal),
        runner.is_enabled(&cancellation),
        runner.is_enabled(&multi_statement),
        runner.is_enabled(&binary_extended),
    ];
    if !enabled.iter().any(|selected| *selected) {
        runner.finish();
        return;
    }

    let setup_started = std::time::Instant::now();
    let runtime = workloads::runtime();
    let fixture = runtime
        .block_on(workloads::unindexed_context(
            "tier4-pgwire-10k",
            FIXTURE_ROWS,
        ))
        .expect("Tier 4 pgwire fixture");
    let preflights = PgwireQueryPreflights::new(&fixture, enabled);
    let context = runtime
        .block_on(workloads::pgwire_transport_for_context(&fixture))
        .expect("Tier 4 pgwire transport context");
    let setup_time_ns = setup_started.elapsed().as_nanos().to_string();

    let PgwireQueryPreflights {
        simple: simple_preflight,
        extended: extended_preflight,
        multi_statement: multi_statement_preflight,
        binary_extended: binary_extended_preflight,
    } = preflights;
    let benchmark = PgwireBenchmark {
        runtime: &runtime,
        fixture: &fixture,
        transport: &context,
        setup_time_ns: &setup_time_ns,
    };
    if enabled[0] {
        benchmark.simple(
            &mut runner,
            simple,
            simple_preflight.expect("enabled simple-query preflight"),
        );
    }
    if enabled[1] {
        benchmark.extended(
            &mut runner,
            extended,
            extended_preflight.expect("enabled extended-query preflight"),
        );
    }
    if enabled[2] {
        benchmark.portal(&mut runner, portal);
    }
    if enabled[3] {
        benchmark.cancellation(&mut runner, cancellation);
    }
    if enabled[4] {
        benchmark.multi_statement(
            &mut runner,
            multi_statement,
            multi_statement_preflight.expect("enabled multi-statement preflight"),
        );
    }
    if enabled[5] {
        benchmark.binary_extended(
            &mut runner,
            binary_extended,
            binary_extended_preflight.expect("enabled binary-query preflight"),
        );
    }
    runtime
        .block_on(context.shutdown())
        .expect("graceful pgwire benchmark shutdown");
    runner.finish();
}

fn declared_case(workload: &str, operation_unit: stress::OperationUnit) -> stress::StressCase {
    stress::StressCase::new(workload, FIXTURE_SCALE).runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Integration,
            FIXTURE_ROWS,
            "tier4_integration_pgwire/10k",
        ),
        operation_unit,
    )
}

struct PgwireBenchmark<'a> {
    runtime: &'a tokio::runtime::Runtime,
    fixture: &'a workloads::BenchContext,
    transport: &'a workloads::PgwireTransportBenchContext,
    setup_time_ns: &'a str,
}

impl PgwireBenchmark<'_> {
    fn simple(
        &self,
        runner: &mut stress::CassieStressRunner,
        case: stress::StressCase,
        preflight: workloads::QueryPreflightEvidence,
    ) {
        runner.record_external(
            query_evidenced(case, self.setup_time_ns, 20, self.fixture, preflight),
            |sample_duration| {
                transport_external::sample_until_deadline(sample_duration, || {
                    let rows = self
                        .runtime
                        .block_on(workloads::pgwire_transport_simple_query(
                            self.transport,
                            workloads::PGWIRE_SIMPLE_QUERY,
                        ));
                    assert_eq!(rows, 20, "simple query result cardinality");
                    1
                })
            },
        );
    }

    fn extended(
        &self,
        runner: &mut stress::CassieStressRunner,
        case: stress::StressCase,
        preflight: workloads::QueryPreflightEvidence,
    ) {
        runner.record_external(
            query_evidenced(case, self.setup_time_ns, 20, self.fixture, preflight),
            |sample_duration| {
                transport_external::sample_until_deadline(sample_duration, || {
                    let rows = self
                        .runtime
                        .block_on(workloads::pgwire_transport_extended_query(self.transport));
                    assert_eq!(rows, 20, "extended query result cardinality");
                    1
                })
            },
        );
    }

    fn portal(&self, runner: &mut stress::CassieStressRunner, case: stress::StressCase) {
        runner.record_external(
            evidenced(case, self.setup_time_ns, 20, self.fixture),
            |sample_duration| {
                transport_external::sample_until_deadline(sample_duration, || {
                    u64::try_from(
                        self.runtime
                            .block_on(workloads::pgwire_transport_portal_fetch(self.transport)),
                    )
                    .expect("portal fetch count should fit u64")
                })
            },
        );
    }

    fn cancellation(&self, runner: &mut stress::CassieStressRunner, case: stress::StressCase) {
        runner.record_external(
            evidenced(case, self.setup_time_ns, 1, self.fixture),
            |sample_duration| {
                transport_external::sample_until_deadline(sample_duration, || {
                    u64::try_from(
                        self.runtime
                            .block_on(workloads::pgwire_transport_cancellation(self.transport)),
                    )
                    .expect("cancellation count should fit u64")
                })
            },
        );
    }

    fn multi_statement(
        &self,
        runner: &mut stress::CassieStressRunner,
        case: stress::StressCase,
        preflight: workloads::QueryPreflightEvidence,
    ) {
        runner.record_external(
            query_evidenced(case, self.setup_time_ns, 2, self.fixture, preflight),
            |sample_duration| {
                transport_external::sample_until_deadline(sample_duration, || {
                    u64::try_from(
                        self.runtime
                            .block_on(workloads::pgwire_transport_multi_statement(self.transport)),
                    )
                    .expect("multi-statement query count should fit u64")
                })
            },
        );
    }

    fn binary_extended(
        &self,
        runner: &mut stress::CassieStressRunner,
        case: stress::StressCase,
        preflight: workloads::QueryPreflightEvidence,
    ) {
        runner.record_external(
            query_evidenced(case, self.setup_time_ns, 20, self.fixture, preflight),
            |sample_duration| {
                transport_external::sample_until_deadline(sample_duration, || {
                    let rows = self
                        .runtime
                        .block_on(workloads::pgwire_transport_binary_query(self.transport));
                    assert_eq!(rows, 20, "binary extended query result cardinality");
                    1
                })
            },
        );
    }
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

struct PgwireQueryPreflights {
    simple: Option<workloads::QueryPreflightEvidence>,
    extended: Option<workloads::QueryPreflightEvidence>,
    multi_statement: Option<workloads::QueryPreflightEvidence>,
    binary_extended: Option<workloads::QueryPreflightEvidence>,
}

impl PgwireQueryPreflights {
    fn new(fixture: &workloads::BenchContext, enabled: [bool; 6]) -> Self {
        Self {
            simple: enabled[0].then(|| {
                workloads::assert_explain_contains(
                    fixture,
                    workloads::PGWIRE_SIMPLE_QUERY,
                    vec![],
                    "access_path=collection_scan",
                )
            }),
            extended: enabled[1].then(|| {
                workloads::assert_explain_contains(
                    fixture,
                    workloads::PGWIRE_EXTENDED_QUERY,
                    vec![cassie::types::Value::String("title-1".to_string())],
                    "access_path=collection_scan",
                )
            }),
            multi_statement: enabled[4].then(|| {
                workloads::assert_explain_contains(
                    fixture,
                    workloads::PGWIRE_MULTI_STATEMENT_COMPONENT_QUERY,
                    vec![],
                    "access_path=collection_scan",
                )
            }),
            binary_extended: enabled[5].then(|| {
                workloads::assert_explain_contains(
                    fixture,
                    workloads::PGWIRE_BINARY_QUERY,
                    vec![cassie::types::Value::Int64(1)],
                    "access_path=collection_scan",
                )
            }),
        }
    }
}
