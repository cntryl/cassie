#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

const SQL_BATCH: usize = 512;

fn main() {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier2-sql-planning", 128))
        .expect("benchmark context");

    let mut runner = stress::runner("tier2_subsystem_sql_planning");
    runner.fixed_batch(
        stress::StressCase::fixed_operations(2, "sql_parsing", "10k")
            .metadata("operation_unit", "sql_statement"),
        logical_operations(SQL_BATCH),
        || repeat(SQL_BATCH, workloads::sql_parsing),
    );
    runner.fixed_batch(
        stress::StressCase::fixed_operations(2, "sql_binding", "10k")
            .metadata("operation_unit", "sql_statement"),
        logical_operations(SQL_BATCH),
        || repeat(SQL_BATCH, || runtime.block_on(workloads::sql_binding(&ctx))),
    );
    runner.fixed_batch(
        stress::StressCase::fixed_operations(2, "logical_planning", "10k")
            .metadata("operation_unit", "sql_statement"),
        logical_operations(SQL_BATCH),
        || {
            repeat(SQL_BATCH, || {
                runtime.block_on(workloads::logical_planning(&ctx))
            })
        },
    );
    runner.fixed_batch(
        stress::StressCase::fixed_operations(2, "physical_planning", "10k")
            .metadata("operation_unit", "sql_statement"),
        logical_operations(SQL_BATCH),
        || {
            repeat(SQL_BATCH, || {
                runtime.block_on(workloads::physical_planning(&ctx))
            })
        },
    );
    runner.finish();
}

fn repeat(mut iterations: usize, mut f: impl FnMut() -> usize) -> usize {
    let mut completed = 0usize;
    while iterations > 0 {
        completed = completed.saturating_add(f());
        iterations -= 1;
    }
    std::hint::black_box(completed)
}

fn logical_operations(iterations: usize) -> u64 {
    u64::try_from(iterations).expect("benchmark batch size should fit u64")
}
