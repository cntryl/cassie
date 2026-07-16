use std::sync::Arc;
use std::time::Instant;

use cassie::app::Cassie;

#[path = "support/performance_benchmarks.rs"]
pub mod performance_benchmarks;
#[path = "support/stress.rs"]
pub mod stress;
#[path = "support/workloads.rs"]
mod workloads;

const OWNER: &str = "tier5_scaling_lifecycle";

fn main() {
    let mut runner = stress::runner(performance_benchmarks::BenchmarkTier::Tier5, OWNER);
    let selected = ["10k", "100k", "250k"].into_iter().any(|scale| {
        ["projection_replay", "projection_rebuild", "startup_reopen"]
            .into_iter()
            .any(|workload| runner.is_enabled(&stress::StressCase::new(workload, scale)))
    }) || [
        "projection_verify",
        "time_series_retention_enforcement",
        "time_series_rollup_refresh",
    ]
    .into_iter()
    .any(|workload| runner.is_enabled(&stress::StressCase::new(workload, "100k")));
    if !selected {
        runner.finish();
        return;
    }
    let runtime = workloads::runtime();
    let mut fixtures = Vec::new();
    for (scale, rows) in [("10k", 10_000), ("100k", 100_000), ("250k", 250_000)] {
        if let Some(fixture) = measure_scale(&runtime, &mut runner, scale, rows) {
            fixtures.push(fixture);
        }
    }
    runner.finish();
    for fixture in fixtures {
        fixture
            .cleanup()
            .expect("clean up lifecycle scaling fixture");
    }
}

struct ScaleCases {
    replay: Option<stress::StressCase>,
    rebuild: Option<stress::StressCase>,
    startup: Option<stress::StressCase>,
    verify: Option<stress::StressCase>,
    retention: Option<stress::StressCase>,
    rollup: Option<stress::StressCase>,
}

impl ScaleCases {
    fn select(runner: &stress::CassieStressRunner, scale: &'static str, rows: usize) -> Self {
        let legacy_100k = scale == "100k";
        Self {
            replay: selected_case(runner, "projection_replay", scale, rows),
            rebuild: selected_case(runner, "projection_rebuild", scale, rows),
            startup: selected_case(runner, "startup_reopen", scale, rows),
            verify: selected_legacy_case(runner, "projection_verify", scale, rows, legacy_100k),
            retention: selected_legacy_case(
                runner,
                "time_series_retention_enforcement",
                scale,
                rows,
                legacy_100k,
            ),
            rollup: selected_legacy_case(
                runner,
                "time_series_rollup_refresh",
                scale,
                rows,
                legacy_100k,
            ),
        }
    }

    fn any_enabled(&self) -> bool {
        self.replay.is_some()
            || self.rebuild.is_some()
            || self.startup.is_some()
            || self.verify.is_some()
            || self.retention.is_some()
            || self.rollup.is_some()
    }
}

fn measure_scale(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    scale: &'static str,
    rows: usize,
) -> Option<workloads::StartupFixture> {
    let cases = ScaleCases::select(runner, scale, rows);
    if !cases.any_enabled() {
        return None;
    }

    let setup_started = Instant::now();
    let context = runtime
        .block_on(workloads::disk_context_with_temp_budget(
            &format!("tier5-lifecycle-{scale}"),
            rows,
            workloads::ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES,
        ))
        .expect("lifecycle scaling fixture");
    let replay_context = cases
        .replay
        .as_ref()
        .map(|_| workloads::isolated_projection_replay_context(&context));
    let mut replay_batches = replay_context
        .as_ref()
        .map(workloads::prepare_isolated_projection_replay_batches);
    if cases.rebuild.is_some() || cases.verify.is_some() {
        workloads::prepare_projection_lifecycle(&context);
    }
    let time_series_context = (cases.retention.is_some() || cases.rollup.is_some())
        .then(|| workloads::prepare_time_series_lifecycle_context(&context, rows));
    let setup_time_ns = setup_started.elapsed().as_nanos().to_string();
    let source_rows = u64::try_from(rows).expect("lifecycle source rows should fit u64");

    if let Some(case) = cases.replay {
        measure_replay(
            runtime,
            runner,
            case,
            &setup_time_ns,
            replay_context.as_ref().expect("isolated replay context"),
            replay_batches.as_mut().expect("prepared replay batches"),
        );
    }
    if let Some(case) = cases.rebuild {
        runner.measure_batch(
            evidenced(case, &setup_time_ns, context.cassie.clone()),
            source_rows,
            || runtime.block_on(workloads::projection_refresh_existing(&context)),
        );
    }
    if let Some(case) = cases.verify {
        runner.measure_batch(
            evidenced(case, &setup_time_ns, context.cassie.clone()),
            source_rows,
            || runtime.block_on(workloads::projection_verify_existing(&context)),
        );
    }
    if let Some(case) = cases.retention {
        measure_retention(
            runtime,
            runner,
            case,
            &setup_time_ns,
            source_rows,
            &context,
            time_series_context
                .as_ref()
                .expect("prepared time-series lifecycle context"),
        );
    }
    if let Some(case) = cases.rollup {
        measure_rollup(
            runtime,
            runner,
            case,
            &setup_time_ns,
            source_rows,
            &context,
            time_series_context
                .as_ref()
                .expect("prepared time-series lifecycle context"),
        );
    }
    workloads::assert_scaling_resource_bounds(&context);
    drop(time_series_context);
    drop(replay_context);
    let fixture = workloads::StartupFixture::from_context(context, rows);
    if let Some(case) = cases.startup {
        runner.measure_batch(base_evidence(case, &setup_time_ns), 1, || fixture.reopen());
    }
    Some(fixture)
}

fn measure_replay(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    case: stress::StressCase,
    setup_time_ns: &str,
    context: &workloads::BenchContext,
    batches: &mut workloads::PreparedProjectionReplayBatches,
) {
    runner.measure_batch(
        evidenced(case, setup_time_ns, context.cassie.clone()),
        u64::try_from(workloads::PROJECTION_REPLAY_EVENTS_PER_BATCH)
            .expect("projection replay event count should fit u64"),
        || {
            let batch = batches.take_next();
            runtime.block_on(workloads::isolated_projection_replay(context, batch))
        },
    );
}

fn measure_retention(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    case: stress::StressCase,
    setup_time_ns: &str,
    source_rows: u64,
    evidence_context: &workloads::BenchContext,
    time_series: &workloads::BenchContext,
) {
    runner.measure_batch(
        evidenced(case, setup_time_ns, evidence_context.cassie.clone()),
        source_rows,
        || {
            let completed =
                runtime.block_on(workloads::time_series_retention_enforcement(time_series));
            assert_eq!(completed, 1, "retention command must report once");
            workloads::assert_scaling_resource_bounds(time_series);
            completed
        },
    );
}

fn measure_rollup(
    runtime: &tokio::runtime::Runtime,
    runner: &mut stress::CassieStressRunner,
    case: stress::StressCase,
    setup_time_ns: &str,
    source_rows: u64,
    evidence_context: &workloads::BenchContext,
    time_series: &workloads::BenchContext,
) {
    runner.measure_batch(
        evidenced(case, setup_time_ns, evidence_context.cassie.clone()),
        source_rows,
        || {
            let completed = runtime.block_on(workloads::time_series_rollup_refresh(time_series));
            assert_eq!(completed, 1, "rollup command must report once");
            workloads::assert_scaling_resource_bounds(time_series);
            completed
        },
    );
}

fn selected_case(
    runner: &stress::CassieStressRunner,
    workload: &'static str,
    scale: &str,
    rows: usize,
) -> Option<stress::StressCase> {
    let operation_unit = match workload {
        "projection_replay" => stress::OperationUnit::Event,
        "startup_reopen" => stress::OperationUnit::Startup,
        "projection_rebuild"
        | "projection_verify"
        | "time_series_retention_enforcement"
        | "time_series_rollup_refresh" => stress::OperationUnit::SourceRow,
        _ => panic!("unsupported lifecycle workload '{workload}'"),
    };
    let case = stress::StressCase::new(workload, scale).runtime_contract(
        stress::FixtureDeclaration::new(
            performance_benchmarks::FixtureClass::Scaling,
            rows,
            format!("{OWNER}/{scale}"),
        ),
        operation_unit,
    );
    runner.is_enabled(&case).then_some(case)
}

fn selected_legacy_case(
    runner: &stress::CassieStressRunner,
    workload: &'static str,
    scale: &str,
    rows: usize,
    enabled: bool,
) -> Option<stress::StressCase> {
    if enabled {
        selected_case(runner, workload, scale, rows)
    } else {
        None
    }
}

fn evidenced(
    case: stress::StressCase,
    setup_time_ns: &str,
    runtime_evidence: Arc<Cassie>,
) -> stress::StressCase {
    base_evidence(case, setup_time_ns).runtime_evidence(runtime_evidence)
}

fn base_evidence(case: stress::StressCase, setup_time_ns: &str) -> stress::StressCase {
    case.metadata("setup_time_ns", setup_time_ns)
        .metadata("execution_result_cache_hits", "0")
        .metadata(
            "query_memory_budget_bytes",
            workloads::ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES.to_string(),
        )
}
