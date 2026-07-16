use super::{
    parse_bool_from, parse_operator_switching_enabled_from, parse_u16_from, parse_u64_from,
    parse_usize_from, parse_usize_min_from, CassieRuntimeLimits, ExecutionResultCacheEnabled,
};

pub(super) fn limits_from_env(
    env_reader: &impl Fn(&str) -> Option<String>,
    defaults: &CassieRuntimeLimits,
) -> CassieRuntimeLimits {
    CassieRuntimeLimits {
        query_timeout_ms: parse_u64_from(
            env_reader,
            "CASSIE_QUERY_TIMEOUT_MS",
            defaults.query_timeout_ms,
        ),
        max_result_rows: parse_usize_from(
            env_reader,
            "CASSIE_MAX_RESULT_ROWS",
            defaults.max_result_rows,
        ),
        cte_recursion_depth: parse_usize_from(
            env_reader,
            "CASSIE_CTE_RECURSION_DEPTH",
            defaults.cte_recursion_depth,
        ),
        query_memory_budget_bytes: parse_usize_from(
            env_reader,
            "CASSIE_QUERY_MEMORY_BUDGET_BYTES",
            defaults.query_memory_budget_bytes,
        ),
        execution_result_cache_enabled: if parse_bool_from(
            env_reader,
            "CASSIE_EXECUTION_RESULT_CACHE_ENABLED",
            defaults.execution_result_cache_enabled.is_enabled(),
        ) {
            ExecutionResultCacheEnabled::enabled()
        } else {
            ExecutionResultCacheEnabled::disabled()
        },
        execution_result_cache_max_entries: parse_usize_from(
            env_reader,
            "CASSIE_EXECUTION_RESULT_CACHE_MAX_ENTRIES",
            defaults.execution_result_cache_max_entries,
        ),
        execution_result_cache_max_bytes: parse_usize_from(
            env_reader,
            "CASSIE_EXECUTION_RESULT_CACHE_MAX_BYTES",
            defaults.execution_result_cache_max_bytes,
        ),
        plan_cache_entries: parse_usize_from(
            env_reader,
            "CASSIE_PLAN_CACHE_ENTRIES",
            defaults.plan_cache_entries,
        ),
        cf2_plan_ttl_seconds: parse_u64_from(
            env_reader,
            "CASSIE_CF2_PLAN_TTL_SECONDS",
            defaults.cf2_plan_ttl_seconds,
        ),
        cf2_plan_candidate_ttl_seconds: parse_u64_from(
            env_reader,
            "CASSIE_CF2_PLAN_CANDIDATE_TTL_SECONDS",
            defaults.cf2_plan_candidate_ttl_seconds,
        ),
        cf2_fulltext_stats_ttl_seconds: parse_u64_from(
            env_reader,
            "CASSIE_CF2_FULLTEXT_STATS_TTL_SECONDS",
            defaults.cf2_fulltext_stats_ttl_seconds,
        ),
        feedback_entries: parse_usize_from(
            env_reader,
            "CASSIE_FEEDBACK_ENTRIES",
            defaults.feedback_entries,
        ),
        feedback_ttl_seconds: parse_u64_from(
            env_reader,
            "CASSIE_FEEDBACK_TTL_SECONDS",
            defaults.feedback_ttl_seconds,
        ),
        operator_feedback_enabled: parse_bool_from(
            env_reader,
            "CASSIE_OPERATOR_FEEDBACK_ENABLED",
            defaults.operator_feedback_enabled,
        ),
        vectorized_joins_enabled: parse_bool_from(
            env_reader,
            "CASSIE_VECTORIZED_JOINS_ENABLED",
            defaults.vectorized_joins_enabled,
        ),
        vectorized_join_batch_size: parse_usize_from(
            env_reader,
            "CASSIE_VECTORIZED_JOIN_BATCH_SIZE",
            defaults.vectorized_join_batch_size,
        ),
        pgwire_max_connections: parse_usize_min_from(
            env_reader,
            "CASSIE_PGWIRE_MAX_CONNECTIONS",
            defaults.pgwire_max_connections,
            1,
        ),
        rest_max_connections: parse_usize_min_from(
            env_reader,
            "CASSIE_REST_MAX_CONNECTIONS",
            defaults.rest_max_connections,
            1,
        ),
        ..adaptive_limits_from_env(env_reader, defaults)
    }
}

fn adaptive_limits_from_env(
    env_reader: &impl Fn(&str) -> Option<String>,
    defaults: &CassieRuntimeLimits,
) -> CassieRuntimeLimits {
    CassieRuntimeLimits {
        adaptive_execution_enabled: parse_bool_from(
            env_reader,
            "CASSIE_ADAPTIVE_EXECUTION_ENABLED",
            defaults.adaptive_execution_enabled,
        ),
        adaptive_min_cost_savings_bps: parse_usize_from(
            env_reader,
            "CASSIE_ADAPTIVE_MIN_COST_SAVINGS_BPS",
            defaults.adaptive_min_cost_savings_bps,
        ),
        adaptive_min_confidence_bps: parse_u16_from(
            env_reader,
            "CASSIE_ADAPTIVE_MIN_CONFIDENCE_BPS",
            defaults.adaptive_min_confidence_bps,
        ),
        operator_switching_enabled: parse_operator_switching_enabled_from(
            env_reader,
            defaults.operator_switching_enabled,
        ),
        operator_switch_join_row_threshold: parse_usize_from(
            env_reader,
            "CASSIE_OPERATOR_SWITCH_JOIN_ROW_THRESHOLD",
            defaults.operator_switch_join_row_threshold,
        ),
        adaptive_candidate_min: parse_usize_from(
            env_reader,
            "CASSIE_ADAPTIVE_CANDIDATE_MIN",
            defaults.adaptive_candidate_min,
        ),
        adaptive_candidate_max: parse_usize_from(
            env_reader,
            "CASSIE_ADAPTIVE_CANDIDATE_MAX",
            defaults.adaptive_candidate_max,
        ),
        parallel_scan_workers: parse_usize_from(
            env_reader,
            "CASSIE_PARALLEL_SCAN_WORKERS",
            defaults.parallel_scan_workers,
        ),
        parallel_scoring_workers: parse_usize_from(
            env_reader,
            "CASSIE_PARALLEL_SCORING_WORKERS",
            defaults.parallel_scoring_workers,
        ),
        parallel_aggregation_workers: parse_usize_from(
            env_reader,
            "CASSIE_PARALLEL_AGGREGATION_WORKERS",
            defaults.parallel_aggregation_workers,
        ),
        max_query_workers: parse_usize_min_from(
            env_reader,
            "CASSIE_MAX_QUERY_WORKERS",
            defaults.max_query_workers,
            1,
        ),
        ..defaults.clone()
    }
}
