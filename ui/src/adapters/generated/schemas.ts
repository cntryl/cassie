export type CollectionCreateRequest = {
  "name": string;
  "description"?: string;
  "fields": Array<FieldSpec>;
};

export type ColumnMeta = {
  "name": string;
  "data_type": string;
  "type_oid": number;
  "typlen": number;
  "atttypmod": number;
  "format_code": number;
  "nullable": boolean;
};

export type ConsistencyCheckRequest = {
  "manifests": Array<{

}>;
};

export type CreateCollectionResponse = {
  "collection": string;
};

export type CreateDocumentResponse = {
  "id": string;
};

export type CreateIndexRequest = {
  "field": string;
  "kind"?: string;
  "options"?: StringMap;
};

export type DeleteDocumentResponse = {
  "deleted": boolean;
};

export type DocumentPayload = {

};

export type Error = {
  "error": string;
};

export type ExportManifestRequest = {
  "instance_id"?: string;
  "generated_ms"?: number;
  "ttl_ms"?: number;
  "include_row_hashes"?: boolean;
};

export type FieldSpec = {
  "name": string;
  "type": string;
};

export type Health = {
  "ready": boolean;
  "status"?: string;
  "metrics"?: {

};
  "collections"?: number;
  "version"?: string;
};

export type ProjectionCheckReport = {
  "report_id": string;
  "created_ms": number;
  "projection_id": string;
  "projection_version_id"?: string | null;
  "state": "consistent" | "divergent" | "stale" | "unverifiable" | "incompatible";
  "compatibility_status": string;
  "manifest_count": number;
  "instance_ids": Array<string>;
  "root_digest"?: string | null;
  "manifest_digest"?: string | null;
  "mismatch_count": number;
  "divergent_range_count": number;
  "divergent_row_count": number;
  "stale_manifest_count": number;
  "incompatible_manifest_count": number;
  "unverifiable_count": number;
  "diagnostic_sample": Array<string>;
  "last_error"?: string | null;
};

export type ProjectionConsistencyReports = {
  "reports": Array<ProjectionCheckReport>;
};

export type ProjectionManifest = {
  "manifest_version": number;
  "instance_id": string;
  "projection_id": string;
  "projection_version_id"?: string | null;
  "projection_kind": string;
  "schema_epoch": number;
  "projection_definition_hash"?: number | null;
  "source_identity"?: string | null;
  "source_checkpoint"?: string | null;
  "source_position"?: number | null;
  "generated_ms": number;
  "expires_at_ms": number;
  "hash": ProjectionManifestHashMetadata;
  "root"?: ProjectionManifestRootSummary | null;
  "ranges": Array<ProjectionManifestRangeSummary>;
  "row_hashes": Array<ProjectionManifestRowHashSummary>;
  "manifest_digest": string;
};

export type ProjectionManifestHashMetadata = {
  "algorithm": string;
  "digest_length": number;
  "canonical_encoder_version": number;
  "row_hash_version": number;
  "range_hash_version": number;
  "root_hash_version": number;
};

export type ProjectionManifestRangeSummary = {
  "range_id": number;
  "first_row_id"?: string | null;
  "last_row_id"?: string | null;
  "row_count": number;
  "digest": string;
  "state": string;
  "computed_ms": number;
};

export type ProjectionManifestRootSummary = {
  "digest": string;
  "row_count": number;
  "range_count": number;
  "state": string;
  "computed_ms": number;
};

export type ProjectionManifestRowHashSummary = {
  "row_id": string;
  "digest": string;
  "state": string;
  "computed_ms": number;
};

export type QueryExecuteRequest = {
  "sql": string;
};

export type QueryExplainPlan = {
  "format_version": number;
  "summary": QueryPlanSummary;
  "nodes": Array<QueryPlanNode>;
  "attributes": Array<QueryPlanAttribute>;
  "estimates": QueryPlanEstimates;
  "features": Array<QueryPlanFeature>;
  "diagnostics": QueryPlanDiagnostics;
  "analyze"?: QueryPlanAnalyze;
};

export type QueryExplainRequest = {
  "sql": string;
};

export type QueryExplainResponse = {
  "columns": Array<ColumnMeta>;
  "rows": Array<Array<QueryResultValue>>;
  "command": string;
  "plan": QueryExplainPlan;
};

export type QueryPlanAnalyze = {
  "actual_rows": number;
  "actual_ms": number;
  "operator_actuals": Array<QueryPlanOperatorActual>;
  "diagnostics": QueryPlanAnalyzeDiagnostics;
};

export type QueryPlanAnalyzeDiagnostics = {
  "plan_cache_hits_delta": number;
  "plan_cache_misses_delta": number;
  "storage_reads_delta": number;
  "storage_writes_delta": number;
  "temp_writes_delta": number;
  "candidate_count_delta": number;
  "result_count_delta": number;
  "parallel_aggregations_delta": number;
  "parallel_aggregation_fallback_delta": number;
  "parallel_aggregation_workers_delta": number;
  "parallel_aggregation_groups_delta": number;
  "adaptive_plan_decisions_delta": number;
  "adaptive_plan_selected_delta": number;
  "operator_switch_attempts_delta": number;
  "operator_switch_success_delta": number;
  "operator_switch_skips_delta": number;
  "operator_switch_fallbacks_delta": number;
};

export type QueryPlanAttribute = {
  "label": string;
  "value": string;
  "intent": string;
};

export type QueryPlanDiagnostics = {
  "access_path_reason": string;
  "fallback_reason": string;
  "pagination_strategy": string;
  "early_stop": string;
  "projection_shape": string;
  "operator_feedback_state": string;
  "operator_feedback_reason": string;
  "adaptive_enabled": boolean;
  "adaptive_decision_point": string;
  "adaptive_candidates": Array<string>;
  "adaptive_selected_alternative": string;
  "adaptive_reason": string;
  "join_strategy": string;
  "join_fallback_reason": string;
  "rollup_rewrite": string;
  "projection_freshness": string;
};

export type QueryPlanEstimates = {
  "scan_rows": number;
  "index_rows": number;
  "join_rows": number;
  "search_rows": number;
  "vector_rows": number;
  "aggregate_rows": number;
  "scan_cost": number;
  "index_cost": number;
  "selected_cost": number;
  "cost_source": string;
  "rejected_alternatives": Array<string>;
};

export type QueryPlanFeature = {
  "id": string;
  "label": string;
  "enabled": boolean;
  "intent": string;
  "detail": string;
  "node_id": string;
};

export type QueryPlanMetric = {
  "label": string;
  "value": string;
  "unit"?: string;
};

export type QueryPlanNode = {
  "id": string;
  "label": string;
  "kind": string;
  "detail": string;
  "status": string;
  "badges": Array<string>;
  "metrics": Array<QueryPlanMetric>;
};

export type QueryPlanOperatorActual = {
  "operator": string;
  "rows_in": number;
  "rows_out": number;
  "elapsed_ms": number;
  "storage_reads": number;
  "storage_writes": number;
  "temp_writes": number;
  "candidates": number;
  "results": number;
};

export type QueryPlanSummary = {
  "collection": string;
  "root_operator": string;
  "access_path": string;
  "selected_index"?: string;
  "selected_cost": number;
  "estimated_rows": number;
  "storage_mode": string;
};

export type QueryResult = {
  "columns": Array<ColumnMeta>;
  "rows": Array<Array<QueryResultValue>>;
  "command": string;
};

export type QueryResultValue = string | number | boolean | Array<QueryResultValue> | {

} | null;

export type QuerySchemaItem = {
  "id": string;
  "kind": "table" | "view" | "index" | "udf" | "procedure";
  "label": string;
  "metadata"?: string;
};

export type QuerySchemaResponse = {
  "sections": Array<QuerySchemaSection>;
};

export type QuerySchemaSection = {
  "id": "tables" | "views" | "indexes" | "udfs" | "procedures";
  "label": string;
  "items": Array<QuerySchemaItem>;
};

export type QueryValidateRequest = {
  "sql": string;
};

export type QueryValidateResponse = {
  "valid": boolean;
  "command": string;
  "columns"?: Array<ColumnMeta>;
};

export type SearchRequest = {
  "field": string;
  "query": string;
  "metric"?: string;
  "limit"?: number;
  "offset"?: number;
};

export type Session = {
  "user": string;
  "database": string;
  "role"?: string;
};

export type StringMap = {
  [key: string]: string;
};

export type VectorIndexResponse = {
  "collection": string;
  "field": string;
  "source_field": string;
  "provider": string;
  "model": string;
  "dimensions": number;
  "metric": string;
  "index_type": string;
  "status": string;
};
