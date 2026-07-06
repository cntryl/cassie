const BENCHMARK: &str = "tier3_system_rebuild";
const REBUILD_TEMP_BUDGET_BYTES: usize = 64 * 1024 * 1024;
const PROJECTION_REBUILD_QUERY_ROWS: u64 = 512;
const SMALL_STATEFUL_REBUILD_REASON: &str = "small_stateful_rebuild_diagnostic";
const STATEFUL_TIME_SERIES_REASON: &str = "stateful_time_series_policy_diagnostic";

#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let mut runner = stress::runner(BENCHMARK);
    let mut index_nonce = 0usize;
    let mut retention_nonce = 0usize;
    let mut rollup_nonce = 0usize;

    bench_projection_rebuild_10k(&mut runner, &runtime, &mut index_nonce);
    bench_projection_rebuild_100k(&mut runner, &runtime);
    bench_time_series_rebuild(
        &mut runner,
        &runtime,
        10_000,
        "tier3-rebuild-ts",
        "10k",
        &mut retention_nonce,
        &mut rollup_nonce,
    );
    bench_time_series_rebuild(
        &mut runner,
        &runtime,
        100_000,
        "tier3-rebuild-ts-100k",
        "100k",
        &mut retention_nonce,
        &mut rollup_nonce,
    );

    runner.finish();
}

fn bench_projection_rebuild_10k(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    index_nonce: &mut usize,
) {
    let cases = [
        stress::StressCase::fixed_operations(3, "projection_rebuild_query", "10k"),
        stress::StressCase::fixed_operations(3, "projection_refresh", "10k"),
        stress::StressCase::fixed_operations(3, "projection_verify", "10k"),
        stress::StressCase::fixed_operations(3, "projection_swap", "10k"),
        stress::StressCase::fixed_operations(3, "index_rebuild_ddl", "10k"),
    ];
    if !cases.iter().any(|case| runner.is_enabled(case)) {
        return;
    }

    let context = runtime
        .block_on(workloads::disk_context_with_temp_budget(
            "tier3-rebuild",
            10_000,
            REBUILD_TEMP_BUDGET_BYTES,
        ))
        .expect("benchmark context");
    runner.fixed_batch(
        cases[0].clone().metadata("operation_unit", "result_row"),
        PROJECTION_REBUILD_QUERY_ROWS,
        || runtime.block_on(workloads::projection_rebuild_query(&context)),
    );

    let refresh = performance_benchmarks::expect_benchmark(BENCHMARK, "projection_refresh", "10k");
    runner.fixed_timed_count(
        stress::StressCase::fixed_operations(3, refresh.workload, refresh.fixture_scale)
            .metadata("operation_unit", "source_row")
            .metadata("signal_role", "informational")
            .metadata("signal_reason", SMALL_STATEFUL_REBUILD_REASON),
        row_count(10_000),
        || runtime.block_on(workloads::projection_refresh_workflow(&context)),
    );

    let verify = performance_benchmarks::expect_benchmark(BENCHMARK, "projection_verify", "10k");
    runner.fixed_timed_count(
        stress::StressCase::fixed_operations(3, verify.workload, verify.fixture_scale)
            .metadata("operation_unit", "source_row")
            .metadata("signal_role", "informational")
            .metadata("signal_reason", SMALL_STATEFUL_REBUILD_REASON),
        row_count(10_000),
        || runtime.block_on(workloads::projection_rebuild_verification(&context)),
    );

    runner.fixed_timed_count(
        cases[3].clone().metadata("operation_unit", "source_row"),
        row_count(10_000),
        || {
            *index_nonce = index_nonce.wrapping_add(1);
            runtime.block_on(workloads::projection_version_swap(&context, *index_nonce))
        },
    );
    runner.fixed_timed_count(
        cases[4].clone().metadata("operation_unit", "source_row"),
        row_count(10_000),
        || {
            *index_nonce = index_nonce.wrapping_add(1);
            runtime.block_on(workloads::index_rebuild_ddl(&context, *index_nonce))
        },
    );
}

fn bench_projection_rebuild_100k(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
) {
    let refresh = performance_benchmarks::expect_benchmark(BENCHMARK, "projection_refresh", "100k");
    let verify = performance_benchmarks::expect_benchmark(BENCHMARK, "projection_verify", "100k");
    let refresh_case =
        stress::StressCase::fixed_operations(3, refresh.workload, refresh.fixture_scale);
    let verify_case =
        stress::StressCase::fixed_operations(3, verify.workload, verify.fixture_scale);
    if !runner.is_enabled(&refresh_case) && !runner.is_enabled(&verify_case) {
        return;
    }

    let context = runtime
        .block_on(workloads::unindexed_disk_context_with_temp_budget(
            "tier3-rebuild-100k",
            100_000,
            REBUILD_TEMP_BUDGET_BYTES,
        ))
        .expect("100k benchmark context");
    runner.fixed_timed_count(
        refresh_case.metadata("operation_unit", "source_row"),
        row_count(100_000),
        || runtime.block_on(workloads::projection_refresh_workflow(&context)),
    );
    runner.fixed_timed_count(
        verify_case.metadata("operation_unit", "source_row"),
        row_count(100_000),
        || runtime.block_on(workloads::projection_rebuild_verification(&context)),
    );
}

fn bench_time_series_rebuild(
    runner: &mut stress::CassieStressRunner,
    runtime: &tokio::runtime::Runtime,
    rows: usize,
    label: &str,
    scale: &str,
    retention_nonce: &mut usize,
    rollup_nonce: &mut usize,
) {
    let retention = performance_benchmarks::expect_benchmark(
        BENCHMARK,
        "time_series_retention_enforcement",
        scale,
    );
    let rollup =
        performance_benchmarks::expect_benchmark(BENCHMARK, "time_series_rollup_refresh", scale);
    let retention_case =
        stress::StressCase::fixed_operations(3, retention.workload, retention.fixture_scale);
    let rollup_case =
        stress::StressCase::fixed_operations(3, rollup.workload, rollup.fixture_scale);
    if !runner.is_enabled(&retention_case) && !runner.is_enabled(&rollup_case) {
        return;
    }

    let context = runtime
        .block_on(workloads::time_series_disk_context_with_temp_budget(
            label,
            rows,
            REBUILD_TEMP_BUDGET_BYTES,
        ))
        .expect("time-series benchmark context");
    runner.fixed_timed_count(
        retention_case
            .metadata("operation_unit", "source_row")
            .metadata("signal_role", "informational")
            .metadata("signal_reason", STATEFUL_TIME_SERIES_REASON),
        row_count(rows),
        || {
            *retention_nonce = retention_nonce.wrapping_add(1);
            runtime.block_on(workloads::time_series_retention_enforcement(
                &context,
                *retention_nonce,
            ))
        },
    );
    runner.fixed_timed_count(
        rollup_case
            .metadata("operation_unit", "source_row")
            .metadata("signal_role", "informational")
            .metadata("signal_reason", STATEFUL_TIME_SERIES_REASON),
        row_count(rows),
        || {
            *rollup_nonce = rollup_nonce.wrapping_add(1);
            runtime.block_on(workloads::time_series_rollup_refresh(
                &context,
                *rollup_nonce,
            ))
        },
    );
}

fn row_count(rows: usize) -> u64 {
    u64::try_from(rows).expect("benchmark row count should fit u64")
}
