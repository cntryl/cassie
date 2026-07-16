use std::time::{Duration, Instant};

use cassie::types::Value;

const PGWIRE_QUERY_SQL: &str =
    "SELECT id, title FROM bench_documents WHERE title = $1 ORDER BY id ASC LIMIT 20";
const HTTP_QUERY_SQL: &str = "SELECT id, title FROM bench_documents ORDER BY id ASC LIMIT 20";
const CLIENT_QUERY_SQL: &str = "SELECT id FROM bench_documents WHERE score >= $1 LIMIT 20";

#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

const OWNER: &str = "tier5_scaling_transport";

struct TransportFixture {
    context: workloads::BenchContext,
    generated_http_tls: Option<workloads::GeneratedHttpTlsMaterial>,
    pgwire: Option<workloads::PgwireTransportBenchContext>,
    client_pool: Option<workloads::PgwireClientPool>,
    http: Option<workloads::HttpBenchContext>,
    setup_time: Duration,
    pgwire_preflight: Option<(workloads::QueryPreflightEvidence, Duration)>,
    http_preflight: Option<(workloads::QueryPreflightEvidence, Duration)>,
    client_preflight: Option<(workloads::QueryPreflightEvidence, Duration)>,
}

struct TransportCases {
    pgwire: (stress::StressCase, bool),
    http: (stress::StressCase, bool),
    clients: Vec<(stress::StressCase, usize, bool)>,
    churn: Option<(stress::StressCase, bool)>,
    legacy_pgwire: Vec<(stress::StressCase, &'static str, bool)>,
    legacy_http: Option<(stress::StressCase, bool)>,
}

struct TransportRequirements {
    query: [bool; 2],
    lifecycle: [bool; 3],
    max_clients: Option<usize>,
}

impl TransportRequirements {
    const fn pgwire(&self) -> bool {
        self.query[0]
    }

    const fn http(&self) -> bool {
        self.query[1]
    }

    const fn churn(&self) -> bool {
        self.lifecycle[0]
    }

    const fn legacy_pgwire(&self) -> bool {
        self.lifecycle[1]
    }

    const fn legacy_http(&self) -> bool {
        self.lifecycle[2]
    }
}

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier5, OWNER);
    let selected = ["10k", "100k", "250k"].into_iter().any(|scale| {
        let scale_selected = ["pgwire_query", "http_query"]
            .into_iter()
            .any(|workload| runner.is_enabled(&stress::StressCase::new(workload, scale)));
        let clients_selected = scale == "100k"
            && [
                "clients_1",
                "clients_2",
                "clients_4",
                "clients_8",
                "clients_16",
            ]
            .into_iter()
            .any(|workload| runner.is_enabled(&stress::StressCase::new(workload, scale)));
        let churn_selected = scale == "10k"
            && runner.is_enabled(&stress::StressCase::new("connection_churn", scale));
        scale_selected || clients_selected || churn_selected
    }) || [
        "pgwire_simple_query",
        "pgwire_multi_statement_query",
        "pgwire_binary_query",
        "pgwire_prepared_query",
        "http_document_create_get",
    ]
    .into_iter()
    .any(|workload| runner.is_enabled(&stress::StressCase::new(workload, "100k")));
    if !selected {
        runner.finish();
        return;
    }
    let runtime = workloads::runtime();
    for (scale, rows) in [("10k", 10_000), ("100k", 100_000), ("250k", 250_000)] {
        measure_scale(&runtime, &mut runner, scale, rows);
    }
    runner.finish();
}

fn measure_scale(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    scale: &'static str,
    rows: usize,
) {
    let cases = select_transport_cases(runner, scale, rows);
    if !cases.any_enabled() {
        return;
    }
    let requirements = cases.requirements();
    let fixture = prepare_transport_fixture(runtime, scale, rows, &requirements);
    let TransportCases {
        pgwire,
        http,
        clients,
        churn,
        legacy_pgwire,
        legacy_http,
    } = cases;
    if pgwire.1 {
        measure_pgwire_query(
            runtime,
            runner,
            pgwire.0,
            &fixture.context,
            fixture.pgwire.as_ref().expect("enabled pgwire listener"),
            fixture
                .pgwire_preflight
                .as_ref()
                .expect("pgwire query preflight"),
        );
    }
    if http.1 {
        measure_http_query(
            runtime,
            runner,
            http.0,
            &fixture.context,
            fixture.http.as_ref().expect("enabled HTTP listener"),
            fixture
                .http_preflight
                .as_ref()
                .expect("HTTP query preflight"),
        );
    }
    if let (Some(client_pool), Some(client_preflight)) =
        (&fixture.client_pool, &fixture.client_preflight)
    {
        measure_client_sweep(
            runtime,
            runner,
            clients,
            &fixture.context,
            client_pool,
            client_preflight,
        );
    }
    if let Some((churn_case, true)) = churn {
        measure_connection_churn(
            runtime,
            runner,
            churn_case,
            fixture.setup_time,
            &fixture.context,
            fixture.pgwire.as_ref().expect("enabled pgwire listener"),
        );
    }
    measure_legacy_pgwire(runtime, runner, legacy_pgwire, &fixture);
    if let Some((legacy_http_case, true)) = legacy_http {
        measure_legacy_http(runtime, runner, legacy_http_case, &fixture);
    }

    shutdown_transport_fixture(
        runtime,
        &fixture.context,
        fixture.http,
        fixture.client_pool,
        fixture.pgwire,
        fixture.generated_http_tls,
    );
}

impl TransportCases {
    fn any_enabled(&self) -> bool {
        self.pgwire.1
            || self.http.1
            || self.clients.iter().any(|(_, _, enabled)| *enabled)
            || self.churn.as_ref().is_some_and(|(_, enabled)| *enabled)
            || self.legacy_pgwire.iter().any(|(_, _, enabled)| *enabled)
            || self
                .legacy_http
                .as_ref()
                .is_some_and(|(_, enabled)| *enabled)
    }

    fn requirements(&self) -> TransportRequirements {
        TransportRequirements {
            query: [self.pgwire.1, self.http.1],
            lifecycle: [
                self.churn.as_ref().is_some_and(|(_, enabled)| *enabled),
                self.legacy_pgwire.iter().any(|(_, _, enabled)| *enabled),
                self.legacy_http
                    .as_ref()
                    .is_some_and(|(_, enabled)| *enabled),
            ],
            max_clients: self
                .clients
                .iter()
                .filter_map(|(_, clients, enabled)| enabled.then_some(*clients))
                .max(),
        }
    }
}

fn select_transport_cases(
    runner: &stress::CassieStressRunner,
    scale: &'static str,
    rows: usize,
) -> TransportCases {
    let pgwire = declared_case("pgwire_query", scale, rows, stress::OperationUnit::Query);
    let http = declared_case("http_query", scale, rows, stress::OperationUnit::Query);
    TransportCases {
        pgwire: (pgwire.clone(), runner.is_enabled(&pgwire)),
        http: (http.clone(), runner.is_enabled(&http)),
        clients: select_client_cases(runner, scale, rows),
        churn: (scale == "10k").then(|| {
            let case = declared_case(
                "connection_churn",
                scale,
                rows,
                stress::OperationUnit::Connection,
            );
            let enabled = runner.is_enabled(&case);
            (case, enabled)
        }),
        legacy_pgwire: select_legacy_pgwire_cases(runner, scale, rows),
        legacy_http: (scale == "100k").then(|| {
            let case = declared_case(
                "http_document_create_get",
                scale,
                rows,
                stress::OperationUnit::Workflow,
            );
            let enabled = runner.is_enabled(&case);
            (case, enabled)
        }),
    }
}

fn select_client_cases(
    runner: &stress::CassieStressRunner,
    scale: &str,
    rows: usize,
) -> Vec<(stress::StressCase, usize, bool)> {
    if scale != "100k" {
        return Vec::new();
    }
    [
        ("clients_1", 1),
        ("clients_2", 2),
        ("clients_4", 4),
        ("clients_8", 8),
        ("clients_16", 16),
    ]
    .into_iter()
    .map(|(workload, clients)| {
        let case = declared_case(workload, scale, rows, stress::OperationUnit::Query);
        let enabled = runner.is_enabled(&case);
        (case, clients, enabled)
    })
    .collect()
}

fn select_legacy_pgwire_cases(
    runner: &stress::CassieStressRunner,
    scale: &str,
    rows: usize,
) -> Vec<(stress::StressCase, &'static str, bool)> {
    if scale != "100k" {
        return Vec::new();
    }
    [
        "pgwire_simple_query",
        "pgwire_multi_statement_query",
        "pgwire_binary_query",
        "pgwire_prepared_query",
    ]
    .into_iter()
    .map(|workload| {
        let case = declared_case(workload, scale, rows, stress::OperationUnit::Query);
        let enabled = runner.is_enabled(&case);
        (case, workload, enabled)
    })
    .collect()
}

fn declared_case(
    workload: &str,
    scale: &str,
    rows: usize,
    operation_unit: stress::OperationUnit,
) -> stress::StressCase {
    stress::StressCase::new(workload, scale).runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Scaling,
            rows,
            format!("{OWNER}/{scale}"),
        ),
        operation_unit,
    )
}

fn prepare_transport_fixture(
    runtime: &tokio::runtime::Runtime,
    scale: &str,
    rows: usize,
    requirements: &TransportRequirements,
) -> TransportFixture {
    let setup_started = Instant::now();
    let generated_http_tls = if requirements.http() || requirements.legacy_http() {
        workloads::configure_http_tls().expect("configure benchmark REST TLS identity")
    } else {
        None
    };
    let context = runtime
        .block_on(workloads::context(
            &format!("tier5-transport-{scale}"),
            rows,
        ))
        .expect("transport scaling fixture");
    let pgwire_needed = requirements.pgwire()
        || requirements.churn()
        || requirements.legacy_pgwire()
        || requirements.max_clients.is_some();
    let pgwire = pgwire_needed.then(|| {
        runtime
            .block_on(workloads::pgwire_transport_for_context(&context))
            .expect("pgwire scaling listener")
    });
    let client_pool = requirements.max_clients.map(|clients| {
        runtime
            .block_on(workloads::pgwire_transport_client_pool(
                pgwire.as_ref().expect("client sweep pgwire listener"),
                clients,
            ))
            .expect("prepare persistent pgwire client pool")
    });
    let http = (requirements.http() || requirements.legacy_http()).then(|| {
        runtime
            .block_on(workloads::http_transport_context(&context))
            .expect("HTTP scaling listener")
    });
    let setup_time = setup_started.elapsed();
    let pgwire_preflight = requirements.pgwire().then(|| {
        query_preflight(
            &context,
            PGWIRE_QUERY_SQL,
            vec![Value::String("title-1".to_string())],
            setup_time,
        )
    });
    let http_preflight = requirements
        .http()
        .then(|| query_preflight(&context, HTTP_QUERY_SQL, vec![], setup_time));
    let client_preflight = requirements.max_clients.map(|_| {
        query_preflight(
            &context,
            CLIENT_QUERY_SQL,
            vec![Value::Int64(0)],
            setup_time,
        )
    });
    TransportFixture {
        context,
        generated_http_tls,
        pgwire,
        client_pool,
        http,
        setup_time,
        pgwire_preflight,
        http_preflight,
        client_preflight,
    }
}

fn measure_pgwire_query(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    case: stress::StressCase,
    context: &workloads::BenchContext,
    pgwire: &workloads::PgwireTransportBenchContext,
    preflight: &(workloads::QueryPreflightEvidence, Duration),
) {
    runner.measure_batch(
        evidenced(case, preflight.1, 20, context, Some(preflight.0.clone())),
        1,
        || runtime.block_on(workloads::pgwire_transport_extended_query(pgwire)),
    );
}

fn measure_http_query(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    case: stress::StressCase,
    context: &workloads::BenchContext,
    http: &workloads::HttpBenchContext,
    preflight: &(workloads::QueryPreflightEvidence, Duration),
) {
    runner.measure_batch(
        evidenced(case, preflight.1, 20, context, Some(preflight.0.clone())),
        1,
        || runtime.block_on(workloads::http_transport_query(http)),
    );
}

fn measure_client_sweep(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    client_cases: Vec<(stress::StressCase, usize, bool)>,
    context: &workloads::BenchContext,
    client_pool: &workloads::PgwireClientPool,
    preflight: &(workloads::QueryPreflightEvidence, Duration),
) {
    for (case, clients, enabled) in client_cases {
        if !enabled {
            continue;
        }
        let result_cardinality = u64::try_from(clients.saturating_mul(20))
            .expect("client result cardinality should fit u64");
        let case = evidenced(
            case,
            preflight.1,
            result_cardinality,
            context,
            Some(preflight.0.clone()),
        )
        .metadata("client_count", clients.to_string());
        runner.measure_batch(case, u64::try_from(clients).expect("client count"), || {
            runtime.block_on(client_pool.query(clients))
        });
    }
}

fn measure_connection_churn(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    case: stress::StressCase,
    setup_time: Duration,
    context: &workloads::BenchContext,
    pgwire: &workloads::PgwireTransportBenchContext,
) {
    runner.measure_batch(evidenced(case, setup_time, 20, context, None), 1, || {
        runtime.block_on(workloads::pgwire_transport_connection_churn(pgwire))
    });
}

fn measure_legacy_pgwire(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    cases: Vec<(stress::StressCase, &'static str, bool)>,
    fixture: &TransportFixture,
) {
    let pgwire = fixture
        .pgwire
        .as_ref()
        .filter(|_| cases.iter().any(|(_, _, enabled)| *enabled));
    let Some(pgwire) = pgwire else {
        return;
    };
    for (case, workload, enabled) in cases {
        if !enabled {
            continue;
        }
        match workload {
            "pgwire_simple_query" => {
                let preflight = query_preflight(
                    &fixture.context,
                    workloads::PGWIRE_SIMPLE_QUERY,
                    vec![],
                    fixture.setup_time,
                );
                runner.measure_batch(
                    evidenced(case, preflight.1, 20, &fixture.context, Some(preflight.0)),
                    1,
                    || {
                        runtime.block_on(workloads::pgwire_transport_simple_query(
                            pgwire,
                            workloads::PGWIRE_SIMPLE_QUERY,
                        ))
                    },
                );
            }
            "pgwire_multi_statement_query" => {
                let preflight = query_preflight(
                    &fixture.context,
                    workloads::PGWIRE_MULTI_STATEMENT_COMPONENT_QUERY,
                    vec![],
                    fixture.setup_time,
                );
                runner.measure_batch(
                    evidenced(case, preflight.1, 2, &fixture.context, Some(preflight.0)),
                    1,
                    || runtime.block_on(workloads::pgwire_transport_multi_statement(pgwire)),
                );
            }
            "pgwire_binary_query" => {
                let preflight = query_preflight(
                    &fixture.context,
                    workloads::PGWIRE_BINARY_QUERY,
                    vec![Value::Int64(1)],
                    fixture.setup_time,
                );
                runner.measure_batch(
                    evidenced(case, preflight.1, 20, &fixture.context, Some(preflight.0)),
                    1,
                    || runtime.block_on(workloads::pgwire_transport_binary_query(pgwire)),
                );
            }
            "pgwire_prepared_query" => {
                let preflight = query_preflight(
                    &fixture.context,
                    workloads::PGWIRE_EXTENDED_QUERY,
                    vec![Value::String("title-1".to_string())],
                    fixture.setup_time,
                );
                runner.measure_batch(
                    evidenced(case, preflight.1, 20, &fixture.context, Some(preflight.0)),
                    1,
                    || runtime.block_on(workloads::pgwire_transport_extended_query(pgwire)),
                );
            }
            other => panic!("unsupported legacy pgwire scaling workload '{other}'"),
        }
    }
}

fn measure_legacy_http(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    case: stress::StressCase,
    fixture: &TransportFixture,
) {
    runner.measure_batch(
        evidenced(case, fixture.setup_time, 3, &fixture.context, None),
        1,
        || {
            runtime.block_on(workloads::http_transport_document_create_get(
                fixture.http.as_ref().expect("enabled legacy HTTP listener"),
            ))
        },
    );
}

fn shutdown_transport_fixture(
    runtime: &tokio::runtime::Runtime,
    context: &workloads::BenchContext,
    http: Option<workloads::HttpBenchContext>,
    client_pool: Option<workloads::PgwireClientPool>,
    pgwire: Option<workloads::PgwireTransportBenchContext>,
    generated_http_tls: Option<workloads::GeneratedHttpTlsMaterial>,
) {
    if let Some(http) = http {
        runtime
            .block_on(http.shutdown())
            .expect("shutdown HTTP scaling listener");
    }
    if let Some(client_pool) = client_pool {
        runtime.block_on(client_pool.shutdown());
    }
    if let Some(pgwire) = pgwire {
        runtime
            .block_on(pgwire.shutdown())
            .expect("shutdown pgwire scaling listener");
    }
    runtime.block_on(workloads::wait_for_pgwire_session_cleanup(context));
    workloads::assert_scaling_resource_bounds(context);
    let metrics = context.cassie.metrics();
    assert_eq!(metrics["runtime"]["running_queries"].as_u64(), Some(0));
    assert_eq!(
        metrics["runtime"]["active_operator_workers"].as_u64(),
        Some(0)
    );
    assert_eq!(metrics["pgwire"]["active_sessions"].as_u64(), Some(0));
    if let Some(material) = generated_http_tls {
        material
            .cleanup()
            .expect("clean up generated REST TLS identity");
    }
}

fn evidenced(
    case: stress::StressCase,
    setup_time: Duration,
    result_cardinality: u64,
    context: &workloads::BenchContext,
    preflight: Option<workloads::QueryPreflightEvidence>,
) -> stress::StressCase {
    let case = case
        .metadata("setup_time_ns", setup_time.as_nanos().to_string())
        .metadata("execution_result_cache_hits", "0")
        .metadata("failed_operations", "0")
        .metadata("result_cardinality", result_cardinality.to_string())
        .runtime_evidence(context.cassie.clone());
    if let Some(preflight) = preflight {
        case.preflight_evidence(preflight.selected_access_path, preflight.fallback_reason)
    } else {
        case
    }
}

fn query_preflight(
    context: &workloads::BenchContext,
    sql: &str,
    params: Vec<Value>,
    fixture_setup: Duration,
) -> (workloads::QueryPreflightEvidence, Duration) {
    let started = Instant::now();
    let evidence = workloads::assert_explain_contains(
        context,
        sql,
        params,
        "collection=postgres.public.bench_documents",
    );
    (evidence, fixture_setup + started.elapsed())
}
