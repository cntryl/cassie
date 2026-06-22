use super::*;

#[derive(Debug, Clone, Serialize, Default)]
pub struct RuntimeSnapshot {
    pub started: bool,
    pub uptime_seconds: u64,
    pub running_queries: u64,
    pub sql_parse_total: u64,
    pub startup_total: u64,
    pub startup_ms_total: u64,
    pub shutdown_total: u64,
    pub catalog_hydration_total: u64,
    pub catalog_hydration_ms_total: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct QuerySnapshot {
    pub count: u64,
    pub latency_ms_total: u64,
    pub rows_returned_total: u64,
    pub errors_total: u64,
    pub errors_by_class: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RestSnapshot {
    pub requests_total: u64,
    pub latency_ms_total: u64,
    pub by_method: BTreeMap<String, u64>,
    pub by_route: BTreeMap<String, u64>,
    pub by_status_class: BTreeMap<String, u64>,
    pub blocking_started_total: BTreeMap<String, u64>,
    pub blocking_completed_total: BTreeMap<String, u64>,
    pub blocking_error_total: BTreeMap<String, u64>,
    pub blocking_join_failed_total: BTreeMap<String, u64>,
    pub blocking_elapsed_ms_total: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PgwireSnapshot {
    pub active_sessions: u64,
    pub sessions_started_total: u64,
    pub sessions_finished_total: u64,
    pub auth_ok_total: u64,
    pub auth_failed_total: u64,
    pub protocol_errors_total: u64,
    pub simple_queries_total: u64,
    pub extended_queries_total: u64,
    pub prepared_statements: u64,
    pub portals: u64,
    pub messages_total: BTreeMap<String, u64>,
    pub blocking_started_total: BTreeMap<String, u64>,
    pub blocking_completed_total: BTreeMap<String, u64>,
    pub blocking_error_total: BTreeMap<String, u64>,
    pub blocking_join_failed_total: BTreeMap<String, u64>,
    pub blocking_elapsed_ms_total: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ExecutionSnapshot {
    pub count: u64,
    pub latency_ms_total: u64,
    pub candidate_count_total: u64,
    pub result_count_total: u64,
    pub normalized_candidate_count_total: u64,
    pub normalized_fallback_count_total: u64,
    pub prefilter_input_candidate_count_total: u64,
    pub prefilter_filtered_candidate_count_total: u64,
    pub prefilter_fallback_count_total: u64,
    pub prefilter_fallback_reasons: BTreeMap<String, u64>,
    pub ivfflat_executions: u64,
    pub ivfflat_lists_total: u64,
    pub ivfflat_probes_total: u64,
    pub ivfflat_exact_reranks_total: u64,
    pub ivfflat_fallbacks: u64,
    pub last_index_kind: String,
    pub last_fallback_reason: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PlanCacheSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub invalidations: u64,
    pub evictions: u64,
    pub entries: u64,
    pub max_entries: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct QueryCacheSnapshot {
    pub l1_hits: u64,
    pub l1_misses: u64,
    pub l2_hits: u64,
    pub l2_misses: u64,
    pub candidate_promotions: u64,
    pub schema_epoch_rejects: u64,
    pub deserialize_rejects: u64,
    pub fulltext_stats_hits: u64,
    pub fulltext_stats_misses: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CardinalitySnapshot {
    pub reads: u64,
    pub writes: u64,
    pub rebuilds: u64,
    pub unavailable: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct FeedbackSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub writes: u64,
    pub evictions: u64,
    pub entries: u64,
    pub max_entries: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AdaptiveCandidateSnapshot {
    pub decisions: u64,
    pub initial_budget_total: u64,
    pub feedback_budget_total: u64,
    pub expansions_total: u64,
    pub final_candidate_count_total: u64,
    pub exhausted_total: u64,
    pub limit_errors_total: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct CoveringIndexSnapshot {
    pub scans: u64,
    pub row_fetches_avoided: u64,
    pub fallback_scans: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ColumnBatchSnapshot {
    pub scans: u64,
    pub row_fetches_avoided: u64,
    pub fallback_scans: u64,
    pub decode_fallbacks: u64,
    pub compressed_bytes_total: u64,
    pub uncompressed_bytes_total: u64,
    pub skipped_segments: u64,
    pub decoded_columns: u64,
    pub row_blob_fetches: u64,
    pub last_fallback_reason: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct TimeSeriesSnapshot {
    pub scans: u64,
    pub fallback_scans: u64,
    pub rows: u64,
    pub buckets_scanned: u64,
    pub buckets_skipped: u64,
    pub last_index: String,
    pub last_fallback_reason: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct AggregateAccelerationSnapshot {
    pub scans: u64,
    pub accelerated_segments: u64,
    pub decoded_fallback_segments: u64,
    pub row_blob_fallbacks: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ParallelScanSnapshot {
    pub scans: u64,
    pub fallback_scans: u64,
    pub workers: u64,
    pub shards: u64,
    pub rows: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ParallelScoringSnapshot {
    pub scorings: u64,
    pub fallback_scorings: u64,
    pub workers: u64,
    pub partitions: u64,
    pub rows: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ParallelAggregationSnapshot {
    pub aggregations: u64,
    pub fallback_aggregations: u64,
    pub workers: u64,
    pub partitions: u64,
    pub rows: u64,
    pub groups: u64,
    pub last_fallback_reason: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RollupSnapshot {
    pub refreshes: u64,
    pub rewrite_hits: u64,
    pub fallback_scans: u64,
    pub stale_fallbacks: u64,
    pub last_rollup: String,
    pub last_fallback_reason: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ProjectionSnapshot {
    pub replay_batches: u64,
    pub replay_events_applied: u64,
    pub replay_duplicates_skipped: u64,
    pub replay_errors: u64,
    pub write_row_puts: u64,
    pub write_row_deletes: u64,
    pub write_index_puts: u64,
    pub write_index_deletes: u64,
    pub write_metadata_puts: u64,
    pub write_metadata_deletes: u64,
    pub write_duplicate_checks: u64,
    pub write_batch_flushes: u64,
    pub write_rebuild_target_puts: u64,
    pub write_activation_metadata_writes: u64,
    pub materialized_builds: u64,
    pub materialized_refreshes: u64,
    pub version_swaps: u64,
    pub stale_marks: u64,
    pub row_hash_updates: u64,
    pub range_hash_updates: u64,
    pub root_hash_updates: u64,
    pub rebuild_verifications: u64,
    pub rebuild_verification_failures: u64,
    pub integrity_verifications: u64,
    pub integrity_verification_failures: u64,
    pub mixed_execution_fallbacks: u64,
    pub mixed_execution_optimized: u64,
    pub last_projection: String,
    pub last_error: String,
    pub last_state: String,
    pub last_fallback_reason: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RetentionSnapshot {
    pub enforcements: u64,
    pub deleted_rows: u64,
    pub skipped_rows: u64,
    pub errors: u64,
    pub last_policy: String,
    pub last_error: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ReadPathSnapshot {
    pub collection_scans: u64,
    pub collection_scan_fields: u64,
    pub point_lookup_scans: u64,
    pub point_lookup_hits: u64,
    pub point_lookup_misses: u64,
    pub last_collection_scan_collection: String,
    pub last_point_lookup_collection: String,
    pub last_point_lookup_hit: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct StorageFamilySnapshot {
    pub reads: u64,
    pub writes: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct StorageSnapshot {
    pub schema: StorageFamilySnapshot,
    pub data: StorageFamilySnapshot,
    pub temp: StorageFamilySnapshot,
    #[serde(rename = "default")]
    pub default_family: StorageFamilySnapshot,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RuntimeMetricsSnapshot {
    pub runtime: RuntimeSnapshot,
    pub query: QuerySnapshot,
    pub rest: RestSnapshot,
    pub pgwire: PgwireSnapshot,
    pub search: ExecutionSnapshot,
    pub vector: ExecutionSnapshot,
    pub hybrid: ExecutionSnapshot,
    pub storage: StorageSnapshot,
    pub plan_cache: PlanCacheSnapshot,
    pub query_cache: QueryCacheSnapshot,
    pub cardinality: CardinalitySnapshot,
    pub feedback: FeedbackSnapshot,
    pub adaptive_candidates: AdaptiveCandidateSnapshot,
    pub covering_indexes: CoveringIndexSnapshot,
    pub column_batches: ColumnBatchSnapshot,
    pub time_series: TimeSeriesSnapshot,
    pub aggregate_acceleration: AggregateAccelerationSnapshot,
    pub parallel_scans: ParallelScanSnapshot,
    pub parallel_scoring: ParallelScoringSnapshot,
    pub parallel_aggregation: ParallelAggregationSnapshot,
    pub rollups: RollupSnapshot,
    pub projections: ProjectionSnapshot,
    pub retention: RetentionSnapshot,
    pub read_paths: ReadPathSnapshot,
}
