use std::time::{Duration, Instant};

use cassie::types::{Value, Vector};

#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

const OWNER: &str = "tier5_scaling_retrieval";

struct TextRetrievalCases {
    fulltext: stress::StressCase,
    hybrid: stress::StressCase,
    cold: Option<stress::StressCase>,
    warm: Option<stress::StressCase>,
    enabled: [bool; 4],
}

struct VectorRetrievalCases {
    exact: stress::StressCase,
    hnsw: stress::StressCase,
    ivf: stress::StressCase,
    enabled: [bool; 3],
}

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier5, OWNER);
    let selected = ["10k", "100k", "250k"].into_iter().any(|scale| {
        [
            "full_text_query",
            "vector_exact_query",
            "vector_hnsw_persisted",
            "vector_ivfflat_persisted",
            "hybrid_query",
        ]
        .into_iter()
        .any(|workload| runner.is_enabled(&stress::StressCase::new(workload, scale)))
    }) || ["100k", "250k"].into_iter().any(|scale| {
        ["full_text_cold", "full_text_warm"]
            .into_iter()
            .any(|workload| runner.is_enabled(&stress::StressCase::new(workload, scale)))
    });
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
    let fulltext = declared_case("full_text_query", scale, rows);
    let exact = declared_case("vector_exact_query", scale, rows);
    let hnsw = declared_case("vector_hnsw_persisted", scale, rows);
    let ivf = declared_case("vector_ivfflat_persisted", scale, rows);
    let hybrid = declared_case("hybrid_query", scale, rows);
    let fulltext_enabled = runner.is_enabled(&fulltext);
    let vector_enabled = [
        runner.is_enabled(&exact),
        runner.is_enabled(&hnsw),
        runner.is_enabled(&ivf),
    ];
    let hybrid_enabled = runner.is_enabled(&hybrid);
    let cold = (scale != "10k").then(|| declared_case("full_text_cold", scale, rows));
    let warm = (scale != "10k").then(|| declared_case("full_text_warm", scale, rows));
    let cold_enabled = cold.as_ref().is_some_and(|case| runner.is_enabled(case));
    let warm_enabled = warm.as_ref().is_some_and(|case| runner.is_enabled(case));
    if !fulltext_enabled
        && !vector_enabled.iter().any(|selected| *selected)
        && !hybrid_enabled
        && !cold_enabled
        && !warm_enabled
    {
        return;
    }

    let mut setup_time = Duration::ZERO;
    let context = accumulate_setup(&mut setup_time, || {
        runtime
            .block_on(workloads::context_with_mock_tei_embeddings(
                &format!("tier5-retrieval-{scale}"),
                rows,
                rows,
            ))
            .expect("retrieval scaling fixture")
    });
    measure_text_retrieval(
        runtime,
        runner,
        &context,
        &mut setup_time,
        TextRetrievalCases {
            fulltext,
            hybrid,
            cold,
            warm,
            enabled: [fulltext_enabled, hybrid_enabled, cold_enabled, warm_enabled],
        },
    );
    measure_vector_retrieval(
        runtime,
        runner,
        &context,
        &mut setup_time,
        rows,
        VectorRetrievalCases {
            exact,
            hnsw,
            ivf,
            enabled: vector_enabled,
        },
    );
}

fn measure_text_retrieval(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    setup_time: &mut Duration,
    cases: TextRetrievalCases,
) {
    let TextRetrievalCases {
        fulltext,
        hybrid,
        cold,
        warm,
        enabled,
    } = cases;
    let temperature_preflight = (enabled[2] || enabled[3]).then(|| {
        accumulate_setup(setup_time, || {
            workloads::assert_explain_contains(
                context,
                workloads::FULLTEXT_SCALING_SQL,
                fulltext_params(),
                "collection=postgres.public.bench_documents",
            )
        })
    });
    if enabled[2] {
        runner.measure_batch(
            evidenced(
                cold.expect("registered cold full-text case"),
                *setup_time,
                context,
                temperature_preflight
                    .as_ref()
                    .expect("cold full-text preflight")
                    .clone(),
            )
            .metadata("retrieval_temperature", "cold"),
            1,
            || runtime.block_on(workloads::full_text_query(context)),
        );
    }
    if enabled[3] {
        accumulate_setup(setup_time, || {
            workloads::prepare_fulltext_warm_state(context);
        });
        runner.measure_batch(
            evidenced(
                warm.expect("registered warm full-text case"),
                *setup_time,
                context,
                temperature_preflight.expect("warm full-text preflight"),
            )
            .metadata("retrieval_temperature", "warm"),
            1,
            || runtime.block_on(workloads::full_text_query(context)),
        );
    }
    if enabled[0] {
        let preflight = accumulate_setup(setup_time, || {
            workloads::assert_explain_contains(
                context,
                workloads::FULLTEXT_SCALING_SQL,
                fulltext_params(),
                "collection=postgres.public.bench_documents",
            )
        });
        runner.measure_batch(
            evidenced(fulltext, *setup_time, context, preflight),
            1,
            || runtime.block_on(workloads::full_text_query(context)),
        );
    }
    if enabled[1] {
        let preflight = accumulate_setup(setup_time, || {
            workloads::assert_explain_contains(
                context,
                workloads::HYBRID_SCALING_SQL,
                hybrid_params(),
                "mixed_execution=true",
            )
        });
        runner.measure_batch(
            evidenced(hybrid, *setup_time, context, preflight),
            1,
            || runtime.block_on(workloads::hybrid_query(context)),
        );
    }
}

fn measure_vector_retrieval(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    setup_time: &mut Duration,
    fixture_rows: usize,
    cases: VectorRetrievalCases,
) {
    if cases.enabled.iter().any(|enabled| *enabled) {
        accumulate_setup(setup_time, || workloads::drop_vector_index(context));
    }
    if cases.enabled[0] {
        let preflight = accumulate_setup(setup_time, || {
            vector_preflight(context, fixture_rows, workloads::VectorAccessPath::Exact)
        });
        runner.measure_batch(
            evidenced(cases.exact, *setup_time, context, preflight),
            1,
            || runtime.block_on(workloads::vector_query(context)),
        );
    }
    if cases.enabled[1] {
        accumulate_setup(setup_time, || workloads::create_hnsw_index(context));
        let preflight = accumulate_setup(setup_time, || {
            vector_preflight(context, fixture_rows, workloads::VectorAccessPath::Hnsw)
        });
        runner.measure_batch(
            evidenced(cases.hnsw, *setup_time, context, preflight),
            1,
            || runtime.block_on(workloads::vector_hnsw_query(context)),
        );
        if cases.enabled[2] {
            accumulate_setup(setup_time, || workloads::drop_vector_index(context));
        }
    }
    if cases.enabled[2] {
        accumulate_setup(setup_time, || {
            workloads::create_ivfflat_index(context);
        });
        let preflight = accumulate_setup(setup_time, || {
            vector_preflight(context, fixture_rows, workloads::VectorAccessPath::IvfFlat)
        });
        runner.measure_batch(
            evidenced(cases.ivf, *setup_time, context, preflight),
            1,
            || runtime.block_on(workloads::vector_ivfflat_query(context)),
        );
    }
}

fn declared_case(workload: &str, scale: &str, rows: usize) -> stress::StressCase {
    stress::StressCase::new(workload, scale).runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Scaling,
            rows,
            format!("{OWNER}/{scale}"),
        ),
        stress::OperationUnit::Query,
    )
}

fn evidenced(
    case: stress::StressCase,
    setup_time: Duration,
    context: &workloads::BenchContext,
    preflight: workloads::QueryPreflightEvidence,
) -> stress::StressCase {
    case.metadata("setup_time_ns", setup_time.as_nanos().to_string())
        .metadata(
            "query_memory_budget_bytes",
            workloads::ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES.to_string(),
        )
        .metadata("execution_result_cache_hits", "0")
        .preflight_evidence(preflight.selected_access_path, preflight.fallback_reason)
        .runtime_evidence(context.cassie.clone())
}

fn accumulate_setup<T>(setup_time: &mut Duration, setup: impl FnOnce() -> T) -> T {
    let started = Instant::now();
    let value = setup();
    *setup_time += started.elapsed();
    value
}

fn vector_preflight(
    context: &workloads::BenchContext,
    fixture_rows: usize,
    access_path: workloads::VectorAccessPath,
) -> workloads::QueryPreflightEvidence {
    workloads::assert_vector_preflight(
        context,
        workloads::VECTOR_SCALING_SQL,
        vector_params(),
        "collection=postgres.public.bench_documents",
        fixture_rows,
        access_path,
    )
}

fn fulltext_params() -> Vec<Value> {
    workloads::fulltext_scaling_params()
}

fn vector_params() -> Vec<Value> {
    vec![Value::Vector(Vector::new(vec![1.0, 0.0, 0.0]))]
}

fn hybrid_params() -> Vec<Value> {
    vec![
        Value::String("alpha".to_string()),
        Value::Vector(Vector::new(vec![1.0, 0.0, 0.0])),
    ]
}
