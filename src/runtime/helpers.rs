use super::*;

pub fn hash_params(params: &[Value]) -> u64 {
    fn hash_value(hasher: &mut std::hash::DefaultHasher, value: &Value) {
        match value {
            Value::Null => 0u8.hash(hasher),
            Value::Bool(v) => {
                1u8.hash(hasher);
                v.hash(hasher);
            }
            Value::Int64(v) => {
                2u8.hash(hasher);
                v.hash(hasher);
            }
            Value::Float64(v) => {
                3u8.hash(hasher);
                v.to_bits().hash(hasher);
            }
            Value::String(v) => {
                4u8.hash(hasher);
                v.hash(hasher);
            }
            Value::Vector(v) => {
                5u8.hash(hasher);
                v.values.len().hash(hasher);
            }
            Value::Json(v) => {
                6u8.hash(hasher);
                v.to_string().hash(hasher);
            }
        }
    }
    let mut hasher = std::hash::DefaultHasher::new();
    for param in params {
        hash_value(&mut hasher, param);
    }
    hasher.finish()
}

pub fn parameter_shape(params: &[Value]) -> Vec<ParameterShape> {
    params.iter().map(parameter_shape_for_value).collect()
}

pub fn sql_fingerprint(statement: &crate::sql::ast::ParsedStatement) -> u64 {
    stable_fingerprint(&statement.statement)
}

pub fn error_class(error: &CassieError) -> &'static str {
    match error {
        CassieError::CollectionNotFound(_) => "collection_not_found",
        CassieError::NotNullViolation { .. } => "not_null_violation",
        CassieError::UniqueViolation { .. } => "unique_violation",
        CassieError::CheckViolation { .. } => "check_violation",
        CassieError::ForeignKeyViolation { .. } => "foreign_key_violation",
        CassieError::Parse(_) => "parse",
        CassieError::Planner(_) => "planner",
        CassieError::Execution(_) => "execution",
        CassieError::InvalidVector(_) => "invalid_vector",
        CassieError::InvalidEmbedding(_) => "invalid_embedding",
        CassieError::EmbeddingUnavailable(_) => "embedding_unavailable",
        CassieError::Unauthorized => "unauthorized",
        CassieError::NotFound(_) => "not_found",
        CassieError::Unsupported(_) => "unsupported",
        CassieError::Storage(_) => "storage",
        CassieError::StorageBootstrap(_) => "storage_bootstrap",
        CassieError::StorageMissingFamily(_) => "storage_missing_family",
        CassieError::StorageRetryable(_) => "storage_retryable",
    }
}

pub(super) fn parameter_shape_for_value(value: &Value) -> ParameterShape {
    match value {
        Value::Null => ParameterShape::Null,
        Value::Bool(_) => ParameterShape::Bool,
        Value::Int64(_) => ParameterShape::Int64,
        Value::Float64(_) => ParameterShape::Float64,
        Value::String(_) => ParameterShape::String,
        Value::Vector(vector) => ParameterShape::Vector(vector.values.len()),
        Value::Json(_) => ParameterShape::Json,
    }
}

pub(super) fn status_class(status: u16) -> String {
    let class = status / 100;
    format!("{class}xx")
}

pub(super) fn duration_ms(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

pub(super) fn adjust_signed(value: &mut u64, delta: isize) {
    if delta.is_negative() {
        *value = value.saturating_sub(delta.unsigned_abs() as u64);
    } else {
        *value = value.saturating_add(delta as u64);
    }
}

pub(super) fn current_time_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn stable_fingerprint<T: Serialize>(value: &T) -> u64 {
    let mut writer = StableFingerprintWriter::default();
    serde_json::to_writer(&mut writer, value).expect("serialize stable fingerprint");
    writer.finish()
}

#[derive(Default)]
struct StableFingerprintWriter {
    state: u64,
}

impl StableFingerprintWriter {
    fn finish(&self) -> u64 {
        self.state
    }
}

impl Write for StableFingerprintWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x100000001b3;

        if self.state == 0 {
            self.state = FNV_OFFSET_BASIS;
        }
        for byte in buf {
            self.state ^= u64::from(*byte);
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn touch(order: &mut VecDeque<PlanCacheKey>, key: &PlanCacheKey) {
    if let Some(position) = order.iter().position(|entry| entry == key) {
        order.remove(position);
    }
    order.push_back(key.clone());
}

pub(super) fn touch_feedback(order: &mut VecDeque<RuntimeFeedbackKey>, key: &RuntimeFeedbackKey) {
    if let Some(position) = order.iter().position(|entry| entry == key) {
        order.remove(position);
    }
    order.push_back(key.clone());
}

pub(super) fn apply_feedback_observation(
    record: &mut RuntimeFeedbackRecord,
    observation: &RuntimeFeedbackObservation,
    now_ms: u64,
) {
    record.executions = record.executions.saturating_add(1);
    record.rows_in_total = record.rows_in_total.saturating_add(observation.rows_in);
    record.rows_out_total = record.rows_out_total.saturating_add(observation.rows_out);
    record.elapsed_ms_total = record
        .elapsed_ms_total
        .saturating_add(observation.elapsed_ms);
    record.storage_reads_total = record
        .storage_reads_total
        .saturating_add(observation.storage_reads);
    record.storage_writes_total = record
        .storage_writes_total
        .saturating_add(observation.storage_writes);
    record.temp_writes_total = record
        .temp_writes_total
        .saturating_add(observation.temp_writes);
    record.candidate_count_total = record
        .candidate_count_total
        .saturating_add(observation.candidate_count);
    record.result_count_total = record
        .result_count_total
        .saturating_add(observation.result_count);
    if let Some(error_class) = observation.error_class.as_ref() {
        record.errors_total = record.errors_total.saturating_add(1);
        record.last_error_class = Some(error_class.clone());
    }
    if record.first_seen_ms == 0 {
        record.first_seen_ms = now_ms;
    }
    record.last_seen_ms = now_ms;
}

pub(super) fn prune_feedback_by_age(
    feedback: &mut RuntimeFeedbackState,
    now_ms: u64,
    ttl_seconds: u64,
) -> u64 {
    if ttl_seconds == 0 {
        return 0;
    }

    let ttl_ms = ttl_seconds.saturating_mul(1_000);
    let mut evictions = 0;
    let expired = feedback
        .entries
        .iter()
        .filter_map(|(key, record)| {
            (now_ms.saturating_sub(record.last_seen_ms) > ttl_ms).then(|| key.clone())
        })
        .collect::<Vec<_>>();

    for key in expired {
        if feedback.entries.remove(&key).is_some() {
            evictions += 1;
        }
        if let Some(position) = feedback.order.iter().position(|entry| entry == &key) {
            feedback.order.remove(position);
        }
    }

    evictions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::logical::LogicalPlan;
    use crate::planner::physical::{Operator, PhysicalPlan};
    use crate::sql::ast::QuerySource;

    fn sample_plan() -> PhysicalPlan {
        PhysicalPlan {
            collection: "bench_documents".to_string(),
            operators: vec![Operator::Scan, Operator::Filter, Operator::Project],
            estimates: Default::default(),
            predicate_pushdown: false,
            projected_scan_fields: Vec::new(),
            scan_limit: None,
            selected_index: None,
            covered_index: false,
            column_batch_index: None,
            top_k: false,
            top_k_limit: None,
            join_strategy: None,
            parallel_aggregate_candidate: false,
            aggregate_acceleration: false,
            access_path: crate::planner::physical::ReadAccessPath::CollectionScan,
            access_path_reason: "sample-plan".to_string(),
            fallback_reason: None,
            pagination_strategy: crate::planner::physical::PaginationStrategy::None,
            top_k_mode: crate::planner::physical::TopKMode::None,
            projection_shape: crate::planner::physical::ProjectionShape::Collection,
            logical: LogicalPlan {
                command: None,
                source: QuerySource::Collection("bench_documents".to_string()),
                collection: "bench_documents".to_string(),
                ctes: Vec::new(),
                distinct: false,
                distinct_on: Vec::new(),
                projection: Vec::new(),
                filter: None,
                group_by: Vec::new(),
                having: None,
                order: Vec::new(),
                limit: Some(20),
                offset: None,
                set: None,
            },
        }
    }

    #[test]
    fn should_reuse_cached_plan_arc_on_lookup() {
        // Arrange
        let runtime = RuntimeState::new(crate::config::CassieRuntimeLimits::default());
        let key = PlanCacheKey {
            sql_fingerprint: 42,
            schema_epoch: 1,
            data_epoch: 2,
            index_feedback_epoch: 3,
            cost_model_version: 1,
            parameter_shape: vec![ParameterShape::Int64],
            mode: ExecutionMode::SimpleQuery,
            database: Some("postgres".to_string()),
        };
        runtime.plan_cache_store(key.clone(), Arc::new(sample_plan()), false);

        // Act
        let first = runtime.plan_cache_lookup(&key).expect("cached plan");
        let second = runtime.plan_cache_lookup(&key).expect("cached plan");

        // Assert
        assert!(Arc::ptr_eq(&first.plan, &second.plan));
    }
}
