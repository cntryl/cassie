#[path = "support/performance_benchmarks.rs"]
mod performance_benchmarks;
#[path = "support/stress.rs"]
mod stress;
#[path = "support/workloads.rs"]
mod workloads;

const LEXER_BATCH: usize = 16_384;
const PARSER_BATCH: usize = 512;

fn main() {
    let mut runner = stress::runner("tier2_subsystem_parser");
    runner.fixed_batch(
        stress::StressCase::fixed_operations(2, "sql_lexer", "10k")
            .metadata("operation_unit", "sql_statement"),
        logical_operations(LEXER_BATCH),
        || repeat(LEXER_BATCH, workloads::sql_lexing),
    );
    runner.fixed_batch(
        stress::StressCase::fixed_operations(2, "sql_parser", "10k")
            .metadata("operation_unit", "sql_statement"),
        logical_operations(PARSER_BATCH),
        || repeat(PARSER_BATCH, workloads::sql_parsing),
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
