#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let runtime = workloads::runtime();
    let ctx = runtime
        .block_on(workloads::context("tier2-sql-planning", 128))
        .expect("benchmark context");

    let mut runner = stress::runner("tier2_subsystem_sql_planning");
    runner.fixed_operations(
        stress::StressCase::fixed_operations(2, "sql_parsing", "10k"),
        workloads::sql_parsing,
    );
    runner.fixed_operations(
        stress::StressCase::fixed_operations(2, "sql_binding", "10k"),
        || runtime.block_on(workloads::sql_binding(&ctx)),
    );
    runner.fixed_operations(
        stress::StressCase::fixed_operations(2, "logical_planning", "10k"),
        || runtime.block_on(workloads::logical_planning(&ctx)),
    );
    runner.fixed_operations(
        stress::StressCase::fixed_operations(2, "physical_planning", "10k"),
        || runtime.block_on(workloads::physical_planning(&ctx)),
    );
    runner.finish();
}
