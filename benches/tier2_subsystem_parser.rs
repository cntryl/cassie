#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

fn main() {
    let mut runner = stress::runner("tier2_subsystem_parser");
    runner.fixed_operations(
        stress::StressCase::fixed_operations(2, "sql_lexer", "10k"),
        workloads::sql_lexing,
    );
    runner.fixed_operations(
        stress::StressCase::fixed_operations(2, "sql_parser", "10k"),
        workloads::sql_parsing,
    );
    runner.finish();
}
