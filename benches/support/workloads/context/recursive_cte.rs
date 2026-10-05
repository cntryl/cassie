use std::future::{ready, Ready};

use cassie::app::CassieError;

use super::{
    context_with_index_options_and_runtime, BenchContext, BenchIndexOptions, BenchmarkStorageMode,
    ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES,
};

// The unchanged depth-six scalar recursive term materializes one million terminal join
// candidates before WHERE rejects them. This benchmark-only profile admits that work.
pub const LARGE_RECURSIVE_CTE_BENCHMARK_QUERY_MEMORY_BYTES: usize = 2 * 1024 * 1024 * 1024;
pub const LARGE_RECURSIVE_CTE_BENCHMARK_RESOURCE_PROFILE: &str =
    "recursive_cte_materialized_fanout_2g";

pub fn recursive_cte_context(
    label: &str,
    recursion_depth: usize,
) -> Ready<Result<BenchContext, CassieError>> {
    ready(recursive_cte_context_now(label, recursion_depth))
}

fn recursive_cte_context_now(
    label: &str,
    recursion_depth: usize,
) -> Result<BenchContext, CassieError> {
    let expected_rows = recursive_cte_expected_rows(recursion_depth);
    let query_memory_budget_bytes = if expected_rows > 100_000 {
        LARGE_RECURSIVE_CTE_BENCHMARK_QUERY_MEMORY_BYTES
    } else {
        ANALYTICAL_BENCHMARK_QUERY_MEMORY_BYTES
    };
    let context = context_with_index_options_and_runtime(
        label,
        0,
        BenchIndexOptions::none(),
        BenchmarkStorageMode::Default,
        |config| {
            config.limits.query_timeout_ms = 0;
            config.limits.cte_recursion_depth = recursion_depth;
            config.limits.max_result_rows = expected_rows;
            config.limits.query_memory_budget_bytes = query_memory_budget_bytes;
        },
    )?;
    context.cassie.execute_sql(
        &context.session,
        "CREATE TABLE recursive_cte_fanout (n INT)",
        vec![],
    )?;
    for _ in 0..10 {
        context.cassie.execute_sql(
            &context.session,
            "INSERT INTO recursive_cte_fanout (n) VALUES (1)",
            vec![],
        )?;
    }
    Ok(context)
}

fn recursive_cte_expected_rows(recursion_depth: usize) -> usize {
    (0..recursion_depth)
        .scan(1_usize, |power, _| {
            let current = *power;
            *power = power.saturating_mul(10);
            Some(current)
        })
        .sum()
}
