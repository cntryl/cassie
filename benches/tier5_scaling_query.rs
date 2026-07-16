use std::time::{Duration, Instant};

use cassie::types::Value;

#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

const OWNER: &str = "tier5_scaling_query";
const SCALE_ROWS: [(&str, usize); 3] = [("10k", 10_000), ("100k", 100_000), ("250k", 250_000)];
const LEGACY_JOIN_CASES: [(&str, &str, usize); 7] = [
    (
        "vectorized_left_join_limited",
        workloads::LEFT_JOIN_SCALING_SQL,
        50,
    ),
    (
        "vectorized_streaming_inner_join",
        workloads::SPARSE_JOIN_SCALING_SQL,
        50,
    ),
    (
        "vectorized_dense_streaming_inner_join",
        workloads::DENSE_JOIN_SCALING_SQL,
        2,
    ),
    (
        "vectorized_indexed_inner_join",
        workloads::INNER_JOIN_50_SCALING_SQL,
        50,
    ),
    (
        "vectorized_right_indexed_inner_join",
        workloads::INNER_JOIN_50_SCALING_SQL,
        50,
    ),
    (
        "vectorized_late_match_inner_join",
        workloads::LATE_MATCH_JOIN_SCALING_SQL,
        50,
    ),
    (
        "vectorized_fanout_inner_join",
        workloads::FANOUT_JOIN_SCALING_SQL,
        500,
    ),
];

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier5, OWNER);
    if !owner_selected(&runner) {
        runner.finish();
        return;
    }
    let runtime = workloads::runtime();

    for (scale, rows) in SCALE_ROWS {
        measure_scale(&runtime, &mut runner, scale, rows);
    }
    runner.finish();
}

fn owner_selected(runner: &stress::CassieStressRunner) -> bool {
    SCALE_ROWS.into_iter().any(|(scale, _)| {
        ["relational_query", "join_query", "column_query"]
            .into_iter()
            .any(|workload| runner.is_enabled(&stress::StressCase::new(workload, scale)))
    }) || [
        "worker_query",
        "worker_query_2",
        "worker_query_4",
        "simple_sql_query",
        "recursive_cte_query",
        "window_frame_query",
        "mixed_direction_scalar_query",
        "expression_index_query",
        "expression_index_range_query",
        "expression_index_order_query",
    ]
    .into_iter()
    .chain(LEGACY_JOIN_CASES.map(|(workload, _, _)| workload))
    .any(|workload| runner.is_enabled(&stress::StressCase::new(workload, "100k")))
}

struct ScaleCases {
    relational: Option<stress::StressCase>,
    join: Option<stress::StressCase>,
    column: Option<stress::StressCase>,
    worker_one: Option<stress::StressCase>,
    simple: Option<stress::StressCase>,
    mixed_direction: Option<stress::StressCase>,
    expression: Option<stress::StressCase>,
    expression_range: Option<stress::StressCase>,
    expression_order: Option<stress::StressCase>,
    recursive_cte: Option<stress::StressCase>,
    window_frame: Option<stress::StressCase>,
    legacy_joins: Vec<(stress::StressCase, &'static str, &'static str, usize)>,
}

impl ScaleCases {
    fn select(runner: &stress::CassieStressRunner, scale: &str, rows: usize) -> Self {
        let legacy = scale == "100k";
        Self {
            relational: selected_case(runner, "relational_query", scale, rows),
            join: selected_case(runner, "join_query", scale, rows),
            column: selected_case(runner, "column_query", scale, rows),
            worker_one: legacy
                .then(|| selected_case(runner, "worker_query", scale, rows))
                .flatten(),
            simple: legacy
                .then(|| selected_case(runner, "simple_sql_query", scale, rows))
                .flatten(),
            mixed_direction: legacy
                .then(|| selected_case(runner, "mixed_direction_scalar_query", scale, rows))
                .flatten(),
            expression: legacy
                .then(|| selected_case(runner, "expression_index_query", scale, rows))
                .flatten(),
            expression_range: legacy
                .then(|| selected_case(runner, "expression_index_range_query", scale, rows))
                .flatten(),
            expression_order: legacy
                .then(|| selected_case(runner, "expression_index_order_query", scale, rows))
                .flatten(),
            recursive_cte: legacy
                .then(|| selected_case(runner, "recursive_cte_query", scale, rows))
                .flatten(),
            window_frame: legacy
                .then(|| selected_case(runner, "window_frame_query", scale, rows))
                .flatten(),
            legacy_joins: if legacy {
                LEGACY_JOIN_CASES
                    .into_iter()
                    .filter_map(|(workload, sql, expected_rows)| {
                        selected_case(runner, workload, scale, rows)
                            .map(|case| (case, workload, sql, expected_rows))
                    })
                    .collect()
            } else {
                Vec::new()
            },
        }
    }

    fn any_enabled(&self) -> bool {
        self.relational.is_some()
            || self.join.is_some()
            || self.column.is_some()
            || self.worker_one.is_some()
            || self.simple.is_some()
            || self.mixed_direction.is_some()
            || self.expression.is_some()
            || self.expression_range.is_some()
            || self.expression_order.is_some()
            || self.recursive_cte.is_some()
            || self.window_frame.is_some()
            || !self.legacy_joins.is_empty()
    }
}

fn measure_scale(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    scale: &'static str,
    rows: usize,
) {
    let cases = ScaleCases::select(runner, scale, rows);
    let worker_reopens = if scale == "100k" {
        [
            (selected_case(runner, "worker_query_2", scale, rows), 2usize),
            (selected_case(runner, "worker_query_4", scale, rows), 4usize),
        ]
    } else {
        [(None, 2), (None, 4)]
    };
    if !cases.any_enabled() && worker_reopens.iter().all(|(case, _)| case.is_none()) {
        return;
    }

    let setup_started = Instant::now();
    let context = if scale == "100k" {
        runtime
            .block_on(workloads::query_scaling_disk_context(
                "tier5-query-100k",
                rows,
                1,
            ))
            .expect("persisted 100k query scaling fixture")
    } else {
        runtime
            .block_on(workloads::query_scaling_context(
                &format!("tier5-query-{scale}"),
                rows,
                1,
            ))
            .expect("query scaling fixture")
    };
    if cases.recursive_cte.is_some() {
        workloads::prepare_recursive_cte_scaling(&context);
    }
    for (_, workload, _, _) in &cases.legacy_joins {
        workloads::prepare_legacy_scaling_join_collection(&context, rows, workload)
            .expect("prepare legacy join collection in shared scaling fixture");
    }
    let fixture_setup = setup_started.elapsed();

    measure_primary_cases(runtime, runner, &context, fixture_setup, &cases);
    if scale == "100k" {
        let fixture = workloads::QueryScalingFixture::close(context, rows);
        measure_dense_join_reopen(
            runner,
            &fixture,
            cases
                .legacy_joins
                .iter()
                .find(|(_, workload, _, _)| *workload == "vectorized_dense_streaming_inner_join")
                .cloned(),
            fixture_setup,
        );
        for (case, workers) in worker_reopens {
            measure_reopened_worker(runtime, runner, &fixture, case, workers, fixture_setup);
        }
        fixture.cleanup().expect("clean up 100k query fixture");
    } else {
        workloads::assert_scaling_resource_bounds(&context);
    }
}

fn measure_primary_cases(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    fixture_setup: Duration,
    cases: &ScaleCases,
) {
    if let Some(case) = cases.relational.clone() {
        let preflight = workloads::assert_explain_contains(
            context,
            workloads::RELATIONAL_SCALING_SQL,
            vec![Value::Int64(40)],
            "bench_documents_score_idx",
        );
        runner.measure_batch(
            evidenced(case, fixture_setup, context, preflight),
            1,
            || runtime.block_on(workloads::relational_query(context)),
        );
    }
    if let Some(case) = cases.join.clone() {
        let preflight = workloads::assert_explain_contains(
            context,
            workloads::JOIN_SCALING_SQL,
            vec![],
            "vectorized_join_candidate=true",
        );
        runner.measure_batch(
            evidenced(case, fixture_setup, context, preflight),
            1,
            || runtime.block_on(workloads::join_query(context)),
        );
    }
    if let Some(case) = cases.column.clone() {
        let preflight = workloads::assert_explain_contains(
            context,
            workloads::COLUMN_SCALING_SQL,
            vec![],
            "aggregate_acceleration=true",
        );
        runner.measure_batch(
            evidenced(case, fixture_setup, context, preflight),
            1,
            || runtime.block_on(workloads::column_query(context)),
        );
    }
    measure_legacy_scalar_cases(runner, context, fixture_setup, cases);
    measure_recursive_cte(
        runtime,
        runner,
        context,
        fixture_setup,
        cases.recursive_cte.clone(),
    );
    measure_window_frame(
        runtime,
        runner,
        context,
        fixture_setup,
        cases.window_frame.clone(),
    );
    for (case, workload, sql, expected_rows) in &cases.legacy_joins {
        if *workload == "vectorized_dense_streaming_inner_join" {
            continue;
        }
        measure_legacy_join(
            runner,
            context,
            fixture_setup,
            case.clone(),
            workload,
            sql,
            *expected_rows,
        );
    }
    if let Some(case) = cases.worker_one.clone() {
        measure_worker_on_context(runtime, runner, context, case, 1, fixture_setup);
    }
}

fn measure_legacy_scalar_cases(
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    setup: Duration,
    cases: &ScaleCases,
) {
    measure_bound_query(
        runner,
        context,
        setup,
        LegacyQueryCase {
            case: cases.simple.clone(),
            sql: workloads::SIMPLE_SCALING_SQL,
            params: vec![Value::String("doc-1".to_string())],
            expected_rows: 1,
            expected_plan: "access_path=point_lookup",
        },
    );
    measure_bound_query(
        runner,
        context,
        setup,
        LegacyQueryCase {
            case: cases.mixed_direction.clone(),
            sql: workloads::MIXED_DIRECTION_SCALING_SQL,
            params: vec![],
            expected_rows: 50,
            expected_plan: "access_path=prefix_scan",
        },
    );
    measure_bound_query(
        runner,
        context,
        setup,
        LegacyQueryCase {
            case: cases.expression.clone(),
            sql: workloads::EXPRESSION_INDEX_SCALING_SQL,
            params: vec![Value::String("title-1".to_string())],
            expected_rows: 50,
            expected_plan: "access_path=index_seek",
        },
    );
    measure_bound_query(
        runner,
        context,
        setup,
        LegacyQueryCase {
            case: cases.expression_range.clone(),
            sql: workloads::EXPRESSION_INDEX_RANGE_SCALING_SQL,
            params: vec![
                Value::String("title-4".to_string()),
                Value::String("title-9".to_string()),
            ],
            expected_rows: 50,
            expected_plan: "access_path=range_scan",
        },
    );
    measure_bound_query(
        runner,
        context,
        setup,
        LegacyQueryCase {
            case: cases.expression_order.clone(),
            sql: workloads::EXPRESSION_INDEX_ORDER_SCALING_SQL,
            params: vec![],
            expected_rows: 50,
            expected_plan: "access_path=ordered_bounded_scan",
        },
    );
}

struct LegacyQueryCase {
    case: Option<stress::StressCase>,
    sql: &'static str,
    params: Vec<Value>,
    expected_rows: usize,
    expected_plan: &'static str,
}

fn measure_bound_query(
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    setup: Duration,
    query: LegacyQueryCase,
) {
    let Some(case) = query.case else {
        return;
    };
    let preflight_started = Instant::now();
    let preflight = workloads::assert_explain_contains(
        context,
        query.sql,
        query.params.clone(),
        query.expected_plan,
    );
    let case = evidenced(
        case,
        setup + preflight_started.elapsed(),
        context,
        preflight,
    );
    runner.measure_batch(case, 1, || {
        workloads::execute_legacy_query(
            context,
            query.sql,
            query.params.clone(),
            query.expected_rows,
        )
    });
}

fn measure_worker_on_context(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    case: stress::StressCase,
    workers: usize,
    setup: Duration,
) {
    let expected_plan = if workers > 1 {
        "aggregate_parallel=true"
    } else {
        "aggregate_parallel=false"
    };
    let preflight_started = Instant::now();
    let preflight = workloads::assert_explain_contains(
        context,
        workloads::WORKER_SCALING_SQL,
        vec![],
        expected_plan,
    );
    let case = evidenced(
        case,
        setup + preflight_started.elapsed(),
        context,
        preflight,
    )
    .metadata("worker_count", workers.to_string());
    runner.measure_batch(case, 1, || {
        runtime.block_on(workloads::worker_query(context, workers))
    });
}

fn measure_reopened_worker(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    fixture: &workloads::QueryScalingFixture,
    case: Option<stress::StressCase>,
    workers: usize,
    fixture_setup: Duration,
) {
    let Some(case) = case else {
        return;
    };
    let reopen_started = Instant::now();
    let context = fixture
        .reopen(workers)
        .expect("reopen persisted query scaling fixture");
    measure_worker_on_context(
        runtime,
        runner,
        &context,
        case,
        workers,
        fixture_setup + reopen_started.elapsed(),
    );
    context.cassie.shutdown();
    drop(context);
}

fn measure_dense_join_reopen(
    runner: &mut stress::CassieStressRunner,
    fixture: &workloads::QueryScalingFixture,
    selected: Option<(stress::StressCase, &'static str, &'static str, usize)>,
    fixture_setup: Duration,
) {
    let Some((case, workload, sql, expected_rows)) = selected else {
        return;
    };
    let reopen_started = Instant::now();
    let context = fixture
        .reopen_dense_stream()
        .expect("reopen persisted dense-stream fixture");
    let preflight =
        workloads::assert_explain_contains(&context, sql, vec![], "vectorized_join_candidate=true");
    let case = evidenced(
        case,
        fixture_setup + reopen_started.elapsed(),
        &context,
        preflight,
    );
    runner.measure_batch(case, 1, || {
        workloads::execute_legacy_join_query(&context, workload, sql, expected_rows)
    });
    context.cassie.shutdown();
    drop(context);
}

fn measure_recursive_cte(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    fixture_setup: Duration,
    case: Option<stress::StressCase>,
) {
    const UPPER_BOUND: usize = 6;
    let Some(case) = case else {
        return;
    };
    let expected_rows = workloads::recursive_cte_result_rows(UPPER_BOUND);
    let statement = workloads::bound_recursive_cte(UPPER_BOUND);
    let preflight_started = Instant::now();
    let preflight = workloads::assert_explain_contains(
        context,
        &statement.sql,
        statement.params,
        "recursive_cte=",
    );
    let case = evidenced(
        case,
        fixture_setup + preflight_started.elapsed(),
        context,
        preflight,
    );
    runner.measure_batch(
        case,
        u64::try_from(expected_rows).expect("recursive CTE row count should fit u64"),
        || {
            let rows = runtime.block_on(workloads::recursive_cte_query(context, UPPER_BOUND));
            workloads::assert_scaling_resource_bounds(context);
            rows
        },
    );
}

fn measure_window_frame(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    fixture_setup: Duration,
    case: Option<stress::StressCase>,
) {
    const EXPECTED_ROWS: usize = 100_000;
    let Some(case) = case else {
        return;
    };
    let preflight_started = Instant::now();
    let preflight = workloads::assert_explain_contains(
        context,
        workloads::WINDOW_FRAME_SCALING_SQL,
        vec![],
        "window_frame=",
    );
    let case = evidenced(
        case,
        fixture_setup + preflight_started.elapsed(),
        context,
        preflight,
    );
    runner.measure_batch(
        case,
        u64::try_from(EXPECTED_ROWS).expect("window row count should fit u64"),
        || {
            let rows = runtime.block_on(workloads::window_frame_query(context, EXPECTED_ROWS));
            workloads::assert_scaling_resource_bounds(context);
            rows
        },
    );
}

fn measure_legacy_join(
    runner: &mut stress::CassieStressRunner,
    context: &workloads::BenchContext,
    fixture_setup: Duration,
    case: stress::StressCase,
    workload: &'static str,
    sql: &'static str,
    expected_rows: usize,
) {
    let preflight_started = Instant::now();
    workloads::activate_legacy_join_variant(context, workload)
        .expect("activate legacy join access path");
    let preflight =
        workloads::assert_explain_contains(context, sql, vec![], "vectorized_join_candidate=true");
    let case = evidenced(
        case,
        fixture_setup + preflight_started.elapsed(),
        context,
        preflight,
    );
    runner.measure_batch(case, 1, || {
        workloads::execute_legacy_join_query(context, workload, sql, expected_rows)
    });
    workloads::deactivate_legacy_join_variant(context, workload)
        .expect("deactivate legacy join access path");
}

fn selected_case(
    runner: &stress::CassieStressRunner,
    workload: &'static str,
    scale: &str,
    rows: usize,
) -> Option<stress::StressCase> {
    let dense_stream_selection = workload == "vectorized_dense_streaming_inner_join";
    let query_memory_budget = if dense_stream_selection {
        4 * 1_024
    } else {
        workloads::ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES
    };
    let operation_unit = match workload {
        "recursive_cte_query" | "window_frame_query" => stress::OperationUnit::ResultRow,
        _ => stress::OperationUnit::Query,
    };
    let case = stress::StressCase::new(workload, scale)
        .runtime_contract(
            stress::FixtureDeclaration::new(
                performance_benchmarks::FixtureClass::Scaling,
                rows,
                format!("{OWNER}/{scale}"),
            ),
            operation_unit,
        )
        .metadata("query_memory_budget_bytes", query_memory_budget.to_string());
    let case = if dense_stream_selection {
        case.metadata("benchmark_resource_profile", "dense_stream_selection_4k")
    } else {
        case
    };
    runner.is_enabled(&case).then_some(case)
}

fn evidenced(
    case: stress::StressCase,
    setup_time: Duration,
    context: &workloads::BenchContext,
    preflight: workloads::QueryPreflightEvidence,
) -> stress::StressCase {
    case.metadata("setup_time_ns", setup_time.as_nanos().to_string())
        .metadata("execution_result_cache_hits", "0")
        .preflight_evidence(preflight.selected_access_path, preflight.fallback_reason)
        .runtime_evidence(context.cassie.clone())
}
